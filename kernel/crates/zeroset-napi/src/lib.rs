//! Node bindings for the ZeroSet kernel (napi-rs).
//!
//! The binding surface is intentionally buffer/JSON oriented so the TypeScript
//! side never pays per-point object allocation.

use napi::bindgen_prelude::{Buffer, Error, Result};
use napi_derive::napi;
use zeroset_core::{presets, stl};

#[napi]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[napi]
pub fn preset_ir(name: String) -> Result<String> {
    let graph = presets::by_name(&name)
        .ok_or_else(|| Error::from_reason(format!("unknown preset `{name}`")))?;
    serde_json::to_string(&graph).map_err(|error| Error::from_reason(error.to_string()))
}

#[napi]
pub fn mesh_stl(preset: String, resolution: u32) -> Result<Buffer> {
    let graph = presets::by_name(&preset)
        .ok_or_else(|| Error::from_reason(format!("unknown preset `{preset}`")))?;
    let grid = graph
        .sample_grid(resolution)
        .map_err(|error| Error::from_reason(error.to_string()))?;
    let mesh = grid
        .surface_nets()
        .map_err(|error| Error::from_reason(error.to_string()))?;
    let mut bytes = Vec::new();
    stl::write_stl_binary(&mesh, &mut bytes)
        .map_err(|error| Error::from_reason(error.to_string()))?;
    Ok(bytes.into())
}

#[napi]
pub fn mesh_binary(preset: String, resolution: u32, feature_angle: Option<f64>) -> Result<Buffer> {
    Ok(zeroset_core::encode_packed_mesh(
        &preset_mesh(&preset, resolution)?,
        feature_angle.unwrap_or(30.0) as f32,
    )
    .into())
}

/// Promise-returning variant that runs meshing on napi's blocking pool so the
/// Node event loop stays responsive for large resolutions.
#[napi]
pub async fn mesh_binary_async(
    preset: String,
    resolution: u32,
    feature_angle: Option<f64>,
) -> Result<Buffer> {
    let bytes = tokio::task::spawn_blocking(move || {
        mesh_binary(preset, resolution, feature_angle).map(|buffer| buffer.to_vec())
    })
    .await
    .map_err(|error| Error::from_reason(format!("mesh task failed: {error}")))??;
    Ok(bytes.into())
}

fn preset_mesh(preset: &str, resolution: u32) -> Result<zeroset_core::Mesh> {
    let graph = presets::by_name(preset)
        .ok_or_else(|| Error::from_reason(format!("unknown preset `{preset}`")))?;
    let grid = graph
        .sample_grid(resolution)
        .map_err(|error| Error::from_reason(error.to_string()))?;
    grid.surface_nets()
        .map_err(|error| Error::from_reason(error.to_string()))
}

#[napi]
pub fn sample(preset: String, x: f64, y: f64, z: f64) -> Result<f64> {
    let graph = presets::by_name(&preset)
        .ok_or_else(|| Error::from_reason(format!("unknown preset `{preset}`")))?;
    Ok(graph.evaluate(zeroset_core::Vec3::new(x as f32, y as f32, z as f32)) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_mesh_is_zsm2_with_features() {
        let bytes = mesh_binary("bracket".to_string(), 8, Some(30.0)).unwrap();
        let bytes: &[u8] = bytes.as_ref();
        assert_eq!(&bytes[..4], b"ZSM2");
        assert!(u32::from_le_bytes(bytes[12..16].try_into().unwrap()) > 0);
    }

    #[test]
    fn high_feature_angle_removes_edges() {
        let bytes = mesh_binary("bracket".to_string(), 8, Some(120.0)).unwrap();
        let bytes: &[u8] = bytes.as_ref();
        assert_eq!(&bytes[..4], b"ZSM2");
        assert_eq!(u32::from_le_bytes(bytes[12..16].try_into().unwrap()), 0);
    }
}
