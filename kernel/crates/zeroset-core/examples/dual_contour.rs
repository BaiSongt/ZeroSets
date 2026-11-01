use std::error::Error;
use std::time::Instant;

use glam::Vec3;
use zeroset_core::{FieldGraph, Mesh, presets, stl};

fn mean_sdf_error(graph: &FieldGraph, mesh: &Mesh) -> f32 {
    if mesh.positions.is_empty() {
        return 0.0;
    }
    let total: f32 = mesh
        .positions
        .iter()
        .map(|position| graph.evaluate(Vec3::from_array(*position)).abs())
        .sum();
    total / mesh.positions.len() as f32
}

fn main() -> Result<(), Box<dyn Error>> {
    let bracket = presets::bracket();
    let grid = bracket.sample_grid(128)?;

    let started = Instant::now();
    let surface = grid.surface_nets()?;
    let surface_time = started.elapsed();

    let started = Instant::now();
    let dual = bracket.dual_contour(128)?;
    let dual_time = started.elapsed();

    for (name, mesh, elapsed) in [
        ("surface nets", &surface, surface_time),
        ("dual contour", &dual, dual_time),
    ] {
        let stats = mesh.stats();
        println!(
            "{name:12} verts={} tris={} closed={} boundary={} volume={:.2} time={elapsed:?}",
            stats.vertices,
            stats.triangles,
            stats.is_closed,
            stats.boundary_edges,
            stats.signed_volume
        );
    }
    stl::write_stl_binary_file("bracket-dual-contour.stl", &dual)?;

    let mut graph = FieldGraph::new();
    let node = graph.rounded_box(Vec3::ZERO, Vec3::splat(1.0), 0.0);
    graph.set_root(node);
    let grid = graph.sample_grid(64)?;
    let surface = grid.surface_nets()?;
    let dual = graph.dual_contour(64)?;
    println!(
        "sharp box    surface_nets_mean|sdf|={:.6} dual_mean|sdf|={:.6}",
        mean_sdf_error(&graph, &surface),
        mean_sdf_error(&graph, &dual)
    );

    Ok(())
}
