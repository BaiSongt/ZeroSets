use std::error::Error;
use std::time::Instant;

use glam::{DVec3, Vec3};
use zeroset_core::{FieldGraph, presets};
use zeroset_gpu::{AdaptiveConfig, GpuSampler, sample_adaptive};

fn main() -> Result<(), Box<dyn Error>> {
    let sampler = GpuSampler::new()?;
    eprintln!(
        "adapter: {} ({})",
        sampler.adapter_name(),
        sampler.backend()
    );

    let graph = presets::bracket();
    let bounds = graph.bounds();

    for depth in [3u32, 4, 5] {
        let config = AdaptiveConfig {
            bounds,
            base_cells: 16,
            max_depth: depth,
            band_cells: 2.0,
            max_tiles: 8192,
        };

        let started = Instant::now();
        let adaptive = sample_adaptive(&sampler, &graph, &config)?;
        let elapsed = started.elapsed();

        let full_cells = 16u64 * (1u64 << depth);
        let full_points = (full_cells + 1).pow(3);
        let ratio = full_points as f64 / adaptive.point_count().max(1) as f64;

        println!(
            "depth {depth}: tiles={} points={} levels={:?} finest={:.4} full={} sparse_ratio={:.2}x time={:?} truncated={}",
            adaptive.tiles.len(),
            adaptive.point_count(),
            adaptive.levels,
            adaptive.finest_cell().unwrap_or_default(),
            full_points,
            ratio,
            elapsed,
            adaptive.truncated
        );
    }

    precision_demo();
    Ok(())
}

fn precision_demo() {
    let center = Vec3::new(10_000.0, 2_000.0, -3_000.0);
    let mut graph = FieldGraph::new();
    let root = graph.sphere(center, 1.0);
    graph.set_root(root);
    let rebased = graph.rebased(center);

    let center64 = DVec3::new(center.x as f64, center.y as f64, center.z as f64);
    let mut naive_max = 0.0f64;
    let mut rebased_max = 0.0f64;

    for index in 0..256 {
        let angle = index as f64 * 0.137;
        let dir = DVec3::new(angle.cos(), angle.sin(), 0.35).normalize();
        let world64 = center64 + dir;
        let world = Vec3::new(world64.x as f32, world64.y as f32, world64.z as f32);
        let local = Vec3::new(dir.x as f32, dir.y as f32, dir.z as f32);

        naive_max = naive_max.max(graph.evaluate(world).abs() as f64);
        rebased_max = rebased_max.max(rebased.evaluate(local).abs() as f64);
    }

    println!(
        "precision at world offset {center:?}: naive_max={naive_max:.3e} rebased_max={rebased_max:.3e}"
    );
}
