use std::error::Error;
use std::time::Instant;

use zeroset_core::{presets, stl};
use zeroset_gpu::{
    AdaptiveConfig, GpuSampler, mesh_adaptive, mesh_adaptive_dc, mesh_adaptive_mixed,
    mesh_adaptive_mixed_dc, mesh_adaptive_refined, mesh_adaptive_refined_dc,
    mesh_adaptive_refined_dc_f64, mesh_adaptive_refined_f64, sample_adaptive,
};

fn main() -> Result<(), Box<dyn Error>> {
    let sampler = GpuSampler::new()?;
    eprintln!(
        "adapter: {} ({})",
        sampler.adapter_name(),
        sampler.backend()
    );

    let graph = presets::bracket();
    let bounds = graph.bounds();

    for depth in [3u32, 4] {
        let config = AdaptiveConfig {
            bounds,
            base_cells: 16,
            max_depth: depth,
            band_cells: 2.0,
            max_tiles: 8192,
        };
        let started = Instant::now();
        let adaptive = sample_adaptive(&sampler, &graph, &config)?;
        let sampled = started.elapsed();
        let started = Instant::now();
        let mesh = mesh_adaptive(&adaptive)?;
        let meshed = started.elapsed();
        let stats = mesh.stats();

        println!(
            "depth {depth}: tiles={} samples={} verts={} tris={} closed={} boundary={} volume={:.2} sample={:?} mesh={:?}",
            adaptive.tiles.len(),
            adaptive.point_count(),
            stats.vertices,
            stats.triangles,
            stats.is_closed,
            stats.boundary_edges,
            stats.signed_volume,
            sampled,
            meshed
        );

        stl::write_stl_binary_file(format!("adaptive-bracket-{depth}.stl"), &mesh)?;
    }

    let config = AdaptiveConfig {
        bounds,
        base_cells: 16,
        max_depth: 3,
        band_cells: 2.0,
        max_tiles: 8192,
    };
    let adaptive = sample_adaptive(&sampler, &graph, &config)?;
    let started = Instant::now();
    let dc = mesh_adaptive_dc(&graph, &adaptive)?;
    let stats = dc.stats();
    println!(
        "adaptive DC depth 3: verts={} tris={} closed={} boundary={} mesh={:?}",
        stats.vertices,
        stats.triangles,
        stats.is_closed,
        stats.boundary_edges,
        started.elapsed()
    );
    stl::write_stl_binary_file("adaptive-bracket-dc.stl", &dc)?;

    let sphere = presets::sphere();
    let config = AdaptiveConfig {
        bounds: sphere.bounds(),
        base_cells: 8,
        max_depth: 2,
        band_cells: 2.0,
        max_tiles: 12,
    };
    let adaptive = sample_adaptive(&sampler, &sphere, &config)?;
    println!(
        "truncated sphere: truncated={} tiles={} cells={:?}",
        adaptive.truncated,
        adaptive.tiles.len(),
        adaptive
            .tiles
            .iter()
            .map(|tile| tile.spec.cell)
            .collect::<Vec<_>>()
    );
    match mesh_adaptive(&adaptive) {
        Ok(mesh) => println!(
            "strict mesh accepted the grid: {} tris",
            mesh.triangle_count()
        ),
        Err(error) => println!("strict mesh error: {error}"),
    }
    let started = Instant::now();
    let mesh = mesh_adaptive_refined(&sphere, &adaptive)?;
    let stats = mesh.stats();
    println!(
        "refined truncated mesh: verts={} tris={} closed={} boundary={} refinement={:?}",
        stats.vertices,
        stats.triangles,
        stats.is_closed,
        stats.boundary_edges,
        started.elapsed()
    );
    let f32_refined = mesh;

    let started = Instant::now();
    let mesh = mesh_adaptive_refined_dc(&sphere, &adaptive)?;
    let stats = mesh.stats();
    println!(
        "refined truncated DC: verts={} tris={} closed={} boundary={} refinement={:?}",
        stats.vertices,
        stats.triangles,
        stats.is_closed,
        stats.boundary_edges,
        started.elapsed()
    );

    let started = Instant::now();
    let mesh = mesh_adaptive_refined_f64(&sphere, &adaptive)?;
    let mesh_f32 = mesh.to_f32_world();
    let stats = mesh_f32.stats();
    let hausdorff = f32_refined.hausdorff_distance(&mesh_f32, 3);
    println!(
        "refined truncated f64: verts={} tris={} closed={} boundary={} hausdorff={hausdorff:.6} refinement={:?}",
        stats.vertices,
        stats.triangles,
        stats.is_closed,
        stats.boundary_edges,
        started.elapsed()
    );
    let f64_sn_f32 = mesh_f32;

    let started = Instant::now();
    let mesh = mesh_adaptive_refined_dc_f64(&sphere, &adaptive)?;
    let mesh_f32 = mesh.to_f32_world();
    let stats = mesh_f32.stats();
    let hausdorff = f64_sn_f32.hausdorff_distance(&mesh_f32, 3);
    println!(
        "refined truncated f64 DC: verts={} tris={} closed={} boundary={} hausdorff={hausdorff:.6} refinement={:?}",
        stats.vertices,
        stats.triangles,
        stats.is_closed,
        stats.boundary_edges,
        started.elapsed()
    );

    let started = Instant::now();
    let mesh = mesh_adaptive_mixed(&sphere, &adaptive)?;
    let stats = mesh.stats();
    let hausdorff = f32_refined.hausdorff_distance(&mesh, 3);
    println!(
        "mixed-level SN: verts={} tris={} closed={} boundary={} nonmanifold={} volume={:.6} hausdorff={hausdorff:.6} time={:?}",
        stats.vertices,
        stats.triangles,
        stats.is_closed,
        stats.boundary_edges,
        stats.non_manifold_edges,
        stats.signed_volume,
        started.elapsed()
    );

    let started = Instant::now();
    let mesh = mesh_adaptive_mixed_dc(&sphere, &adaptive)?;
    let stats = mesh.stats();
    let hausdorff = f32_refined.hausdorff_distance(&mesh, 3);
    println!(
        "mixed-level DC: verts={} tris={} closed={} boundary={} nonmanifold={} volume={:.6} hausdorff={hausdorff:.6} time={:?}",
        stats.vertices,
        stats.triangles,
        stats.is_closed,
        stats.boundary_edges,
        stats.non_manifold_edges,
        stats.signed_volume,
        started.elapsed()
    );

    Ok(())
}
