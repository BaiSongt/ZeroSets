use std::error::Error;

use zeroset_blocks::{BlockStatus, Notebook, ParamValue, Unit, Value};
use zeroset_core::stl;

fn main() -> Result<(), Box<dyn Error>> {
    let mut notebook = Notebook::new();

    let base = notebook.add_block("box", "Base")?;
    notebook.set_param(
        base,
        "half",
        ParamValue::vector(30.0, 20.0, 6.0, Unit::Millimeter),
    )?;
    notebook.set_param(base, "rounding", ParamValue::number(2.0, Unit::Millimeter))?;

    let mut body = base;
    for (index, y) in [-10.0f32, 0.0, 10.0].into_iter().enumerate() {
        let rib = notebook.add_block("capsule", format!("Rib {index}"))?;
        notebook.set_param(
            rib,
            "a",
            ParamValue::vector(-24.0, y, 6.0, Unit::Millimeter),
        )?;
        notebook.set_param(rib, "b", ParamValue::vector(24.0, y, 6.0, Unit::Millimeter))?;
        notebook.set_param(rib, "radius", ParamValue::number(2.0, Unit::Millimeter))?;

        let blended = notebook.add_block("smooth_union", format!("Blend {index}"))?;
        notebook.set_input(blended, "a", body)?;
        notebook.set_input(blended, "b", rib)?;
        notebook.set_param(blended, "k", ParamValue::number(1.5, Unit::Millimeter))?;
        body = blended;
    }

    let mut hole_index = 0;
    for x in [-18.0f32, 18.0] {
        for y in [-10.0f32, 10.0] {
            let hole = notebook.add_block("cylinder", format!("Hole {hole_index}"))?;
            notebook.set_param(
                hole,
                "center",
                ParamValue::vector(x, y, 0.0, Unit::Millimeter),
            )?;
            notebook.set_param(
                hole,
                "axis",
                ParamValue::vector(0.0, 0.0, 1.0, Unit::Unitless),
            )?;
            notebook.set_param(hole, "radius", ParamValue::number(3.5, Unit::Millimeter))?;
            notebook.set_param(
                hole,
                "half_height",
                ParamValue::number(10.0, Unit::Millimeter),
            )?;

            let cut = notebook.add_block("subtract", format!("Cut {hole_index}"))?;
            notebook.set_input(cut, "a", body)?;
            notebook.set_input(cut, "b", hole)?;
            body = cut;
            hole_index += 1;
        }
    }

    let mesh_block = notebook.add_block("mesh", "Mesh")?;
    notebook.set_input(mesh_block, "field", body)?;
    notebook.set_param(
        mesh_block,
        "resolution",
        ParamValue::number(64.0, Unit::Unitless),
    )?;

    let value = notebook.evaluate(mesh_block)?;
    let mesh = match value {
        Value::Mesh(mesh) => mesh,
        _ => return Err("expected a mesh".into()),
    };
    let stats = mesh.stats();
    println!(
        "blocks={} vertices={} triangles={} closed={}",
        notebook.instances().count(),
        stats.vertices,
        stats.triangles,
        stats.is_closed
    );
    for instance in notebook.instances() {
        println!(
            "  {:>8} {:<14} {:?}",
            instance.id, instance.name, instance.status
        );
    }

    stl::write_stl_binary_file("notebook-bracket.stl", &mesh)?;

    // A parameter edit marks only the block and its consumers dirty; the next
    // evaluation re-meshes and produces a new Arc.
    notebook.set_param(base, "rounding", ParamValue::number(3.0, Unit::Millimeter))?;
    assert_eq!(
        notebook.instance(mesh_block).unwrap().status,
        BlockStatus::Unbuilt
    );
    let rebuilt = match notebook.evaluate(mesh_block)? {
        Value::Mesh(mesh) => mesh,
        _ => return Err("expected a mesh".into()),
    };
    println!(
        "after edit: dirty propagation ok, {} triangles, new mesh={}",
        rebuilt.triangle_count(),
        !std::sync::Arc::ptr_eq(&mesh, &rebuilt)
    );

    Ok(())
}
