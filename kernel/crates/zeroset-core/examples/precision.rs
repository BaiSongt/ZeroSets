use glam::{DVec3, Vec3};
use zeroset_core::FieldGraph;

fn main() {
    let center = Vec3::new(1_000_000.0, 200_000.0, -300_000.0);
    let mut graph = FieldGraph::new();
    let sphere = graph.sphere(center, 1.0);
    graph.set_root(sphere);

    let point = DVec3::new(
        center.x as f64 + 0.03,
        center.y as f64 - 0.04,
        center.z as f64 + 0.05,
    );
    let expected =
        (point - DVec3::new(center.x as f64, center.y as f64, center.z as f64)).length() - 1.0;

    let world = Vec3::new(point.x as f32, point.y as f32, point.z as f32);
    let world_value = graph.evaluate(world) as f64;

    let rebased = graph.rebased(center);
    let local = Vec3::new(
        (point.x - center.x as f64) as f32,
        (point.y - center.y as f64) as f32,
        (point.z - center.z as f64) as f32,
    );
    let local_value = rebased.evaluate(local) as f64;

    let reference = graph.evaluate_f64(point);

    println!("sphere center {center:?}, radius 1, query {point:?}");
    println!("expected          {expected:.12}");
    println!(
        "f32 world         {world_value:.12}  error {:.3e}",
        (world_value - expected).abs()
    );
    println!(
        "f32 rebased(local){local_value:.12}  error {:.3e}",
        (local_value - expected).abs()
    );
    println!(
        "f64 reference     {reference:.12}  error {:.3e}",
        (reference - expected).abs()
    );
}
