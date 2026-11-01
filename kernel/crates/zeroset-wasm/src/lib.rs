//! Browser bindings for the ZeroSet kernel.
//!
//! The public surface stays small and buffer-oriented:
//!
//! ```text
//! preset_ir(name)            -> JSON field DAG
//! mesh_binary(ir, resolution)-> packed mesh (header + f32 + u32)
//! mesh_stats(ir, resolution) -> JSON diagnostics
//! sample(ir, x, y, z)        -> scalar value
//! ```
//!
//! Packed mesh layout, little-endian:
//! `magic "ZSM2" | u32 vertices | u32 triangles | u32 feature_edges | positions f32x3 | normals f32x3 | indices u32x3 | feature edges u32x2`.

use wasm_bindgen::prelude::*;
use zeroset_blocks::{BlockRegistry, Notebook, ParamValue, Unit, Value};
use zeroset_core::{FieldGraph, Mesh, Vec3, presets};

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[wasm_bindgen]
pub fn presets() -> Vec<String> {
    presets::PRESETS
        .iter()
        .map(|name| (*name).to_string())
        .collect()
}

#[wasm_bindgen]
pub fn preset_ir(name: &str) -> Result<String, JsValue> {
    let graph = presets::by_name(name)
        .ok_or_else(|| JsValue::from_str(&format!("unknown preset `{name}`")))?;
    serde_json::to_string(&graph).map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen]
pub fn mesh_binary(ir: &str, resolution: u32) -> Result<Vec<u8>, JsValue> {
    let graph = parse_graph(ir)?;
    let grid = graph
        .sample_grid(resolution)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let mesh = grid
        .surface_nets()
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    Ok(encode_mesh(&mesh))
}

#[wasm_bindgen]
pub fn mesh_stats(ir: &str, resolution: u32) -> Result<String, JsValue> {
    let graph = parse_graph(ir)?;
    let grid = graph
        .sample_grid(resolution)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let mesh = grid
        .surface_nets()
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let stats = mesh.stats();
    serde_json::to_string(&stats).map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen]
pub fn sample(ir: &str, x: f64, y: f64, z: f64) -> Result<f64, JsValue> {
    let graph = parse_graph(ir)?;
    Ok(graph.evaluate(Vec3::new(x as f32, y as f32, z as f32)) as f64)
}

/// Builtin block definitions as JSON: id, name, category, description, and
/// overloads with typed inputs/defaults.
#[wasm_bindgen]
pub fn block_catalog() -> Result<String, JsValue> {
    let registry = BlockRegistry::builtin();
    serde_json::to_string(registry.defs()).map_err(js_error)
}

/// Custom Field expression presets shown in the editor: `[{name, expression}]`.
#[wasm_bindgen]
pub fn custom_templates() -> Result<String, JsValue> {
    serde_json::to_string(&zeroset_blocks::custom_templates()).map_err(js_error)
}

/// Starter Notebook snapshot: sphere + box union, meshed at resolution 64.
#[wasm_bindgen]
pub fn notebook_template() -> Result<String, JsValue> {
    let mut notebook = Notebook::new();

    let sphere = notebook.add_block("sphere", "Sphere").map_err(js_error)?;
    notebook
        .set_param(
            sphere,
            "center",
            ParamValue::vector(-12.0, 0.0, 0.0, Unit::Millimeter),
        )
        .map_err(js_error)?;
    notebook
        .set_param(sphere, "radius", ParamValue::number(12.0, Unit::Millimeter))
        .map_err(js_error)?;

    let block = notebook.add_block("box", "Box").map_err(js_error)?;
    notebook
        .set_param(
            block,
            "center",
            ParamValue::vector(12.0, 0.0, 0.0, Unit::Millimeter),
        )
        .map_err(js_error)?;
    notebook
        .set_param(
            block,
            "half",
            ParamValue::vector(8.0, 8.0, 8.0, Unit::Millimeter),
        )
        .map_err(js_error)?;
    notebook
        .set_param(block, "rounding", ParamValue::number(1.0, Unit::Millimeter))
        .map_err(js_error)?;

    let union = notebook.add_block("union", "Union").map_err(js_error)?;
    notebook.set_input(union, "a", sphere).map_err(js_error)?;
    notebook.set_input(union, "b", block).map_err(js_error)?;

    let mesh = notebook.add_block("mesh", "Mesh").map_err(js_error)?;
    notebook.set_input(mesh, "field", union).map_err(js_error)?;
    notebook
        .set_param(mesh, "resolution", ParamValue::number(64.0, Unit::Unitless))
        .map_err(js_error)?;

    notebook.to_json().map_err(js_error)
}

/// Evaluate a Notebook snapshot and return the packed ZSM1 mesh.
#[wasm_bindgen]
pub fn notebook_eval(snapshot: &str, instance: u64) -> Result<Vec<u8>, JsValue> {
    let mut notebook = Notebook::from_json(snapshot).map_err(js_error)?;
    let value = notebook.evaluate(instance).map_err(js_error)?;
    let feature_angle = notebook
        .number_param(instance, "feature_angle")
        .unwrap_or(30.0f32.to_radians())
        .to_degrees();
    match value {
        Value::Mesh(mesh) => Ok(encode_mesh_with_features(&mesh, feature_angle)),
        other => Err(JsValue::from_str(&format!(
            "instance {instance} produced {}, not Mesh",
            other.value_type()
        ))),
    }
}

/// Evaluate a Notebook snapshot and return its diagnostics as JSON.
#[wasm_bindgen]
pub fn notebook_diagnostics(snapshot: &str, instance: u64) -> Result<String, JsValue> {
    let mut notebook = Notebook::from_json(snapshot).map_err(js_error)?;
    let _ = notebook.evaluate(instance);
    let diagnostics = notebook
        .instance(instance)
        .map(|instance| instance.diagnostics.clone())
        .unwrap_or_default();
    serde_json::to_string(&diagnostics).map_err(js_error)
}

fn parse_graph(ir: &str) -> Result<FieldGraph, JsValue> {
    let graph: FieldGraph =
        serde_json::from_str(ir).map_err(|error| JsValue::from_str(&error.to_string()))?;
    graph
        .validate()
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    Ok(graph)
}

fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

pub fn encode_mesh(mesh: &Mesh) -> Vec<u8> {
    encode_mesh_with_features(mesh, 30.0)
}

pub fn encode_mesh_with_features(mesh: &Mesh, feature_angle: f32) -> Vec<u8> {
    zeroset_core::encode_packed_mesh(mesh, feature_angle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_mesh_has_expected_header() {
        let ir = preset_ir("sphere").unwrap();
        let bytes = mesh_binary(&ir, 16).unwrap();
        assert_eq!(&bytes[..4], b"ZSM2");
        let vertices = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let triangles = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let features = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        assert!(vertices > 0);
        assert!(triangles > 0);
        assert_eq!(
            bytes.len(),
            16 + vertices as usize * 24 + triangles as usize * 12 + features as usize * 8
        );
    }

    #[test]
    fn notebook_diagnostics_include_custom_location() {
        let mut notebook = Notebook::new();
        let custom = notebook.add_block("custom", "Custom").unwrap();
        notebook
            .set_param(
                custom,
                "expression",
                ParamValue::Text("let a = 1;\n".to_string()),
            )
            .unwrap();
        let snapshot = notebook.to_json().unwrap();
        let diagnostics = notebook_diagnostics(&snapshot, custom).unwrap();
        let diagnostics: serde_json::Value = serde_json::from_str(&diagnostics).unwrap();
        let first = &diagnostics[0];
        assert_eq!(first["severity"], "error");
        assert_eq!(first["source"].as_u64(), Some(custom));
        assert_eq!(first["line"], 2);
        assert_eq!(first["column"], 1);
        assert!(
            first["snippet"]
                .as_str()
                .is_some_and(|snippet| snippet.starts_with("2 | "))
        );
    }

    #[test]
    fn custom_templates_are_json() {
        let templates = custom_templates().unwrap();
        let templates: Vec<serde_json::Value> = serde_json::from_str(&templates).unwrap();
        assert!(templates.len() >= 8);
        assert!(templates.iter().all(|template| {
            template["name"]
                .as_str()
                .is_some_and(|name| !name.is_empty())
                && template["expression"]
                    .as_str()
                    .is_some_and(|expression| !expression.is_empty())
        }));
        let offset = templates
            .iter()
            .find(|template| template["name"] == "Offset shell")
            .expect("offset shell template");
        let params = offset["params"].as_array().expect("offset params");
        assert_eq!(params[0][0], "offset");
        assert_eq!(params[0][1]["Number"]["value"], 2.0);
        assert_eq!(params[0][1]["Number"]["unit"], "millimeter");
    }

    #[test]
    fn catalog_and_template_roundtrip() {
        let catalog = block_catalog().unwrap();
        assert!(catalog.contains("\"sphere\""));
        assert!(catalog.contains("\"overloads\""));

        let template = notebook_template().unwrap();
        assert!(template.contains("\"union\""));
        assert!(template.contains("\"mesh\""));
        let mesh = notebook_eval(&template, 4).unwrap();
        assert_eq!(&mesh[..4], b"ZSM2");
        let triangles = u32::from_le_bytes(mesh[8..12].try_into().unwrap());
        let features = u32::from_le_bytes(mesh[12..16].try_into().unwrap());
        assert!(triangles > 0);
        assert!(features > 0);
    }
}
