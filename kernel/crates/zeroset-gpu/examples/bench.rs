use std::hint::black_box;
use std::time::Instant;

use zeroset_core::{Vec3, presets};
use zeroset_gpu::GpuSampler;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let resolution = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(128);

    let graph = presets::bracket();
    let grid = graph.sample_grid(resolution)?;
    let [nx, ny, nz] = grid.dims;
    let count = nx * ny * nz;

    let mut points: Vec<[f32; 3]> = Vec::with_capacity(count);
    for z in 0..nz {
        for y in 0..ny {
            for x in 0..nx {
                points.push(grid.position(x, y, z).to_array());
            }
        }
    }

    let sampler = GpuSampler::new()?;
    eprintln!(
        "adapter: {} ({})",
        sampler.adapter_name(),
        sampler.backend()
    );

    // Warm up shader compilation and buffer allocation before timing.
    let warmup = points.len().min(4096);
    let _ = sampler.sample(&graph, &points[..warmup])?;

    let started = Instant::now();
    let gpu_values = sampler.sample(&graph, &points)?;
    let gpu_time = started.elapsed();

    let started = Instant::now();
    let mut cpu_values = Vec::with_capacity(points.len());
    for point in &points {
        cpu_values.push(graph.evaluate(Vec3::from_array(*point)));
    }
    let cpu_time = started.elapsed();

    let mut max_error = 0.0f32;
    let mut sum_error = 0.0f64;
    for (cpu, gpu) in cpu_values.iter().zip(&gpu_values) {
        let error = (cpu - gpu).abs();
        max_error = max_error.max(error);
        sum_error += error as f64;
    }
    let mean_error = sum_error / cpu_values.len() as f64;

    let gpu_rate = points.len() as f64 / gpu_time.as_secs_f64();
    let cpu_rate = points.len() as f64 / cpu_time.as_secs_f64();

    println!("preset        : bracket");
    println!("resolution    : {resolution} ({nx} x {ny} x {nz} corners)");
    println!("points        : {}", points.len());
    println!("gpu           : {:?} ({gpu_rate:.0} points/s)", gpu_time);
    println!("cpu (single)  : {:?} ({cpu_rate:.0} points/s)", cpu_time);
    println!(
        "speedup       : {:.2}x",
        cpu_time.as_secs_f64() / gpu_time.as_secs_f64()
    );
    println!("max |cpu-gpu| : {max_error:.6}");
    println!("mean |cpu-gpu|: {mean_error:.6}");

    black_box(cpu_values);
    black_box(gpu_values);

    Ok(())
}
