use std::error::Error;
use std::fs::File;
use std::io::BufWriter;
use std::time::Instant;

use zeroset_core::presets;
use zeroset_gpu::{Camera, GpuRaymarchRenderer, RenderConfig};

fn main() -> Result<(), Box<dyn Error>> {
    let renderer = GpuRaymarchRenderer::new()?;
    eprintln!(
        "adapter: {} ({})",
        renderer.adapter_name(),
        renderer.backend()
    );

    let graph = presets::bracket();
    let bounds = graph.bounds();
    let center = bounds.center();
    let radius = bounds.size().length() * 0.5;
    let camera = Camera::looking_at(
        center + glam::Vec3::new(radius * 1.5, radius * 1.0, radius * 1.7),
        center,
    )
    .with_fov(42.0);

    let config = RenderConfig {
        width: 1280,
        height: 800,
        resolution_scale: 1.0,
        max_steps: 160,
        step_scale: 0.9,
        epsilon: 0.0,
    };

    // Warm up shader compilation and driver caches before timing the passes.
    let mut warmup = config;
    warmup.resolution_scale = 0.25;
    let _ = renderer.render(&graph, &camera, &warmup)?;

    let started = Instant::now();
    let progressive = renderer.render_progressive(&graph, &camera, &config, &[0.25, 0.5, 1.0])?;
    let elapsed = started.elapsed();

    for pass in &progressive.passes {
        println!(
            "pass scale={:.2} internal={}x{} time={:?}",
            pass.scale, pass.internal_width, pass.internal_height, pass.elapsed
        );
    }
    println!(
        "final {}x{} total={:?}",
        progressive.image.width, progressive.image.height, elapsed
    );

    let file = File::create("raymarch.png")?;
    let mut encoder = png::Encoder::new(
        BufWriter::new(file),
        progressive.image.width,
        progressive.image.height,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&progressive.image.rgba)?;
    println!("saved raymarch.png");

    Ok(())
}
