use std::error::Error;

use zeroset_blocks::{Notebook, ParamValue, Unit, Value};

fn main() -> Result<(), Box<dyn Error>> {
    let mut notebook = Notebook::new();
    let sphere = notebook.add_block("sphere", "Sphere")?;
    notebook.set_param(sphere, "radius", ParamValue::number(10.0, Unit::Millimeter))?;

    let cube = notebook.add_block("box", "Cube")?;
    notebook.set_param(
        cube,
        "half",
        ParamValue::vector(8.0, 8.0, 8.0, Unit::Millimeter),
    )?;
    notebook.set_param(cube, "rounding", ParamValue::number(1.0, Unit::Millimeter))?;

    let union = notebook.add_block("union", "Union")?;
    notebook.set_input(union, "a", sphere)?;
    notebook.set_input(union, "b", cube)?;

    let mesh = notebook.add_block("mesh", "Mesh")?;
    notebook.set_input(mesh, "field", union)?;
    notebook.set_param(mesh, "resolution", ParamValue::number(40.0, Unit::Unitless))?;

    let value = notebook.evaluate(mesh)?;
    let triangles = match value {
        Value::Mesh(mesh) => mesh.triangle_count(),
        _ => 0,
    };
    println!(
        "original: {} blocks, {} triangles",
        notebook.instances().count(),
        triangles
    );

    let json = notebook.to_json()?;
    std::fs::write("notebook-demo.json", &json)?;
    println!("saved notebook-demo.json ({} bytes)", json.len());

    let mut restored = Notebook::from_json(&json)?;
    let replayed = match restored.evaluate(mesh)? {
        Value::Mesh(mesh) => mesh,
        _ => return Err("expected mesh after reload".into()),
    };
    println!(
        "restored: {} blocks, {} triangles, closed={}",
        restored.instances().count(),
        replayed.triangle_count(),
        replayed.stats().is_closed
    );

    restored.set_param(sphere, "radius", ParamValue::number(15.0, Unit::Millimeter))?;
    let rebuilt = match restored.evaluate(mesh)? {
        Value::Mesh(mesh) => mesh,
        _ => return Err("expected mesh after edit".into()),
    };
    println!(
        "after radius edit: {} triangles (dirty propagation works)",
        rebuilt.triangle_count()
    );

    Ok(())
}
