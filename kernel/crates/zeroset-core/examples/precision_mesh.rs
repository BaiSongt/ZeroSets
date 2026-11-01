use glam::{DVec3, Vec3};
use zeroset_core::FieldGraph;

fn error_stats(mesh: &zeroset_core::Mesh, center: Vec3) -> (f32, f32) {
    let mut max = 0.0f32;
    let mut sum = 0.0f32;
    for position in &mesh.positions {
        let error = ((Vec3::from_array(*position) - center).length() - 1.0).abs();
        max = max.max(error);
        sum += error;
    }
    (max, sum / mesh.positions.len() as f32)
}

fn main() {
    let center = Vec3::new(1_000_000.0, 200_000.0, -300_000.0);
    let mut graph = FieldGraph::new();
    let sphere = graph.sphere(center, 1.0);
    graph.set_root(sphere);

    let f32_mesh = graph.sample_grid(64).unwrap().surface_nets().unwrap();
    let (f32_max, f32_mean) = error_stats(&f32_mesh, center);
    let f32_dc = graph.dual_contour(64).unwrap();
    let (f32_dc_max, f32_dc_mean) = error_stats(&f32_dc, center);

    let f64_grid = graph.sample_grid_f64(64).unwrap();
    let reference = DVec3::new(center.x as f64, center.y as f64, center.z as f64);
    let f64_world = f64_grid.surface_nets_world().unwrap();
    let mut f64_world_max = 0.0f64;
    let mut f64_world_sum = 0.0f64;
    for position in &f64_world.positions {
        let error = (DVec3::from_array(*position).distance(reference) - 1.0).abs();
        f64_world_max = f64_world_max.max(error);
        f64_world_sum += error;
    }
    let f64_world_mean = f64_world_sum / f64_world.positions.len() as f64;

    let f64_world_dc = f64_grid
        .dual_contour_world_with_gradient(|point| graph.gradient_f64(point))
        .unwrap();
    let mut f64_world_dc_max = 0.0f64;
    let mut f64_world_dc_sum = 0.0f64;
    for position in &f64_world_dc.positions {
        let error = (DVec3::from_array(*position).distance(reference) - 1.0).abs();
        f64_world_dc_max = f64_world_dc_max.max(error);
        f64_world_dc_sum += error;
    }
    let f64_world_dc_mean = f64_world_dc_sum / f64_world_dc.positions.len() as f64;
    let f64_mesh = f64_grid.surface_nets_local(reference).unwrap();
    let (f64_max, f64_mean) = error_stats(&f64_mesh, Vec3::ZERO);
    let f64_dc = f64_grid
        .dual_contour_local_with_gradient(reference, |point| graph.gradient_f64(point))
        .unwrap();
    let (f64_dc_max, f64_dc_mean) = error_stats(&f64_dc, Vec3::ZERO);

    let mut f32_local = f32_mesh.clone();
    for position in &mut f32_local.positions {
        position[0] -= center.x;
        position[1] -= center.y;
        position[2] -= center.z;
    }
    let local_hausdorff = f32_local.hausdorff_distance(&f64_mesh, 3);
    let sn_dc_hausdorff = f64_mesh.hausdorff_distance(&f64_dc, 3);

    println!("sphere at {center:?}, radius 1, resolution 64");
    println!(
        "f32 world SN: max {:.6}, mean {:.6}, tris {}",
        f32_max,
        f32_mean,
        f32_mesh.triangle_count()
    );
    println!(
        "f32 world DC: max {:.6}, mean {:.6}, tris {}",
        f32_dc_max,
        f32_dc_mean,
        f32_dc.triangle_count()
    );
    println!(
        "f64 world SN: max {:.9}, mean {:.9}, tris {}",
        f64_world_max,
        f64_world_mean,
        f64_world.triangle_count()
    );
    println!(
        "f64 world DC: max {:.9}, mean {:.9}, tris {}",
        f64_world_dc_max,
        f64_world_dc_mean,
        f64_world_dc.triangle_count()
    );
    println!(
        "f64 local SN: max {:.6}, mean {:.6}, tris {}, closed {}",
        f64_max,
        f64_mean,
        f64_mesh.triangle_count(),
        f64_mesh.stats().is_closed
    );
    println!(
        "f64 local DC: max {:.6}, mean {:.6}, tris {}, closed {}",
        f64_dc_max,
        f64_dc_mean,
        f64_dc.triangle_count(),
        f64_dc.stats().is_closed
    );
    println!(
        "Hausdorff f32-local SN vs f64-local SN: {:.6}",
        local_hausdorff
    );
    println!("Hausdorff f64 SN vs f64 DC: {:.6}", sn_dc_hausdorff);
}
