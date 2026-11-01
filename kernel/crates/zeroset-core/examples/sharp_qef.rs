use zeroset_core::{FieldGraph, Vec3};

fn main() {
    let mut graph = FieldGraph::new();
    let node = graph.rounded_box(Vec3::ZERO, Vec3::splat(1.0), 0.0);
    graph.set_root(node);
    let grid = graph.sample_grid(64).unwrap();
    let surface = grid.surface_nets().unwrap();
    let dual = grid
        .dual_contour_with_gradient(|point| graph.gradient(point))
        .unwrap();

    for (name, mesh) in [("surface nets", &surface), ("dual contouring", &dual)] {
        let mut total = 0.0f64;
        let mut maximum = 0.0f64;
        for position in &mesh.positions {
            let error = graph.evaluate(Vec3::from_array(*position)).abs() as f64;
            total += error;
            maximum = maximum.max(error);
        }
        let mean = total / mesh.positions.len() as f64;
        println!(
            "{name}: verts={} tris={} mean_error={mean:.8} max_error={maximum:.8}",
            mesh.vertex_count(),
            mesh.triangle_count()
        );
    }
}
