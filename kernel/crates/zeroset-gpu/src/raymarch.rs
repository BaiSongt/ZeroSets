use std::time::{Duration, Instant};

use glam::Vec3;

use zeroset_core::FieldGraph;

use crate::{GpuError, compile_raymarch_shader};

/// Simple pinhole camera in world space.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub position: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov_y_degrees: f32,
}

impl Camera {
    pub fn looking_at(position: Vec3, target: Vec3) -> Self {
        Self {
            position,
            target,
            up: Vec3::Y,
            fov_y_degrees: 45.0,
        }
    }

    pub fn with_fov(mut self, degrees: f32) -> Self {
        self.fov_y_degrees = degrees;
        self
    }
}

/// Viewport raymarch settings.
#[derive(Clone, Copy, Debug)]
pub struct RenderConfig {
    pub width: u32,
    pub height: u32,
    /// Internal render scale in `(0, 1]`. The result is upsampled to width/height.
    pub resolution_scale: f32,
    pub max_steps: u32,
    /// Sphere-tracing step multiplier; 0.8-0.95 is a good range.
    pub step_scale: f32,
    /// Base surface tolerance in local units; 0 derives it from model size.
    pub epsilon: f32,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            resolution_scale: 1.0,
            max_steps: 128,
            step_scale: 0.9,
            epsilon: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RenderOutput {
    pub width: u32,
    pub height: u32,
    pub internal_width: u32,
    pub internal_height: u32,
    pub rgba: Vec<u8>,
    pub elapsed: Duration,
}

#[derive(Clone, Copy, Debug)]
pub struct RenderPass {
    pub scale: f32,
    pub internal_width: u32,
    pub internal_height: u32,
    pub elapsed: Duration,
}

#[derive(Clone, Debug)]
pub struct ProgressiveRender {
    pub image: RenderOutput,
    pub passes: Vec<RenderPass>,
}

struct PreparedScene {
    local_graph: FieldGraph,
    camera_position: Vec3,
    forward: Vec3,
    right: Vec3,
    up: Vec3,
    tan_half_fov: f32,
    epsilon: f32,
    max_distance: f32,
}

/// Headless wgpu raymarch renderer.
///
/// The graph is rebased to its bounds center before upload, and the camera is
/// transformed into the same local frame. This keeps ray positions small even
/// when the model is far from the world origin.
pub struct GpuRaymarchRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_name: String,
    backend: String,
}

impl GpuRaymarchRenderer {
    pub fn new() -> Result<Self, GpuError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            ..Default::default()
        }))
        .map_err(|error| GpuError::NoAdapter(error.to_string()))?;

        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("zeroset-raymarch"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|error| GpuError::Device(error.to_string()))?;

        Ok(Self {
            device,
            queue,
            adapter_name: info.name,
            backend: format!("{:?}", info.backend),
        })
    }

    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    pub fn backend(&self) -> &str {
        &self.backend
    }

    /// Render one frame at `config.resolution_scale`.
    pub fn render(
        &self,
        graph: &FieldGraph,
        camera: &Camera,
        config: &RenderConfig,
    ) -> Result<RenderOutput, GpuError> {
        validate_size(config)?;
        let scene = self.prepare(graph, camera)?;
        let pipeline = self.compile_pipeline(&scene.local_graph)?;
        self.render_prepared(&scene, config, &pipeline, true)
    }

    /// Render several resolution scales in order and keep the last image.
    ///
    /// The shader is compiled once for the whole sequence. This is the simplest
    /// dynamic-resolution schedule: a fast coarse pass first, then refinement to
    /// full resolution. The caller can stop early if a frame budget is exceeded.
    pub fn render_progressive(
        &self,
        graph: &FieldGraph,
        camera: &Camera,
        config: &RenderConfig,
        scales: &[f32],
    ) -> Result<ProgressiveRender, GpuError> {
        if scales.is_empty() {
            return Err(GpuError::InvalidRender(
                "progressive rendering needs at least one scale".to_string(),
            ));
        }

        validate_size(config)?;
        let scene = self.prepare(graph, camera)?;
        let pipeline = self.compile_pipeline(&scene.local_graph)?;

        let mut passes = Vec::with_capacity(scales.len());
        let mut last = None;
        for (index, scale) in scales.iter().enumerate() {
            let mut frame = *config;
            frame.resolution_scale = *scale;
            validate_size(&frame)?;
            let is_final = index + 1 == scales.len();
            let output = self.render_prepared(&scene, &frame, &pipeline, is_final)?;
            passes.push(RenderPass {
                scale: *scale,
                internal_width: output.internal_width,
                internal_height: output.internal_height,
                elapsed: output.elapsed,
            });
            last = Some(output);
        }

        Ok(ProgressiveRender {
            image: last.expect("scales is non-empty"),
            passes,
        })
    }

    fn prepare(&self, graph: &FieldGraph, camera: &Camera) -> Result<PreparedScene, GpuError> {
        validate_camera(camera)?;

        let bounds = graph.bounds();
        if bounds.is_empty() || !bounds.is_finite() {
            return Err(GpuError::InvalidRender(
                "field bounds are empty or non-finite".to_string(),
            ));
        }

        let reference = bounds.center();
        let local_graph = graph.rebased(reference);
        let camera_position = camera.position - reference;
        let camera_target = camera.target - reference;

        let forward = (camera_target - camera_position).normalize_or_zero();
        if forward.length_squared() < 0.5 {
            return Err(GpuError::InvalidRender(
                "camera position and target must differ".to_string(),
            ));
        }
        let right = forward.cross(camera.up).normalize_or_zero();
        if right.length_squared() < 0.5 {
            return Err(GpuError::InvalidRender(
                "camera up vector must not be parallel to the view direction".to_string(),
            ));
        }
        let up = right.cross(forward).normalize_or_zero();

        let fov = camera.fov_y_degrees.to_radians();
        let tan_half_fov = (fov * 0.5).tan();
        let model_scale = bounds.max_extent().max(1.0);

        Ok(PreparedScene {
            local_graph,
            camera_position,
            forward,
            right,
            up,
            tan_half_fov,
            epsilon: (model_scale * 1e-4).max(1e-5),
            max_distance: model_scale * 8.0 + camera_position.length() + 1.0,
        })
    }

    fn compile_pipeline(
        &self,
        local_graph: &FieldGraph,
    ) -> Result<wgpu::ComputePipeline, GpuError> {
        let shader_source = compile_raymarch_shader(local_graph)?;
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("zeroset_raymarch"),
                source: wgpu::ShaderSource::Wgsl(shader_source.into()),
            });

        Ok(self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("zeroset_raymarch_pipeline"),
                layout: None,
                module: &shader,
                entry_point: Some("zs_render"),
                compilation_options: Default::default(),
                cache: None,
            }))
    }

    fn render_prepared(
        &self,
        scene: &PreparedScene,
        config: &RenderConfig,
        pipeline: &wgpu::ComputePipeline,
        upscale: bool,
    ) -> Result<RenderOutput, GpuError> {
        let started = Instant::now();
        let layout = pipeline.get_bind_group_layout(0);

        let internal_width =
            ((config.width as f32 * config.resolution_scale).round() as u32).max(1);
        let internal_height =
            ((config.height as f32 * config.resolution_scale).round() as u32).max(1);
        let pixel_angle = 2.0 * scene.tan_half_fov / internal_height as f32;

        let pixel_count = internal_width as usize * internal_height as usize;
        let byte_len = (pixel_count * 4) as u64;

        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zeroset_raymarch_pixels"),
            size: byte_len,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let staging_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zeroset_raymarch_staging"),
            size: byte_len,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut uniforms = [0.0f32; 28];
        uniforms[0..3].copy_from_slice(&scene.camera_position.to_array());
        uniforms[4..7].copy_from_slice(&scene.forward.to_array());
        uniforms[8..11].copy_from_slice(&scene.right.to_array());
        uniforms[12..15].copy_from_slice(&scene.up.to_array());
        uniforms[16] = internal_width as f32;
        uniforms[17] = internal_height as f32;
        uniforms[18] = scene.tan_half_fov;
        uniforms[19] = pixel_angle;
        uniforms[20] = config.max_steps.max(8) as f32;
        uniforms[21] = if config.epsilon > 0.0 && config.epsilon.is_finite() {
            config.epsilon
        } else {
            scene.epsilon
        };
        uniforms[22] = scene.max_distance;
        uniforms[23] = config.step_scale.clamp(0.1, 1.0);
        uniforms[26] = 1.0;

        let uniform_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zeroset_raymarch_uniforms"),
            size: 112,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue
            .write_buffer(&uniform_buffer, 0, bytemuck::cast_slice(&uniforms));

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("zeroset_raymarch_bind_group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("zeroset_raymarch_encoder"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("zeroset_raymarch_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(internal_width.div_ceil(8), internal_height.div_ceil(8), 1);
        }
        encoder.copy_buffer_to_buffer(&output_buffer, 0, &staging_buffer, 0, byte_len);
        self.queue.submit(Some(encoder.finish()));

        let slice = staging_buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| GpuError::Poll(error.to_string()))?;
        receiver
            .recv()
            .map_err(|error| GpuError::Mapping(error.to_string()))?
            .map_err(|error| GpuError::Mapping(error.to_string()))?;

        let view = slice
            .get_mapped_range()
            .map_err(|error| GpuError::Mapping(error.to_string()))?;
        let mut pixels = Vec::with_capacity(pixel_count * 4);
        for chunk in view.chunks_exact(4) {
            let packed = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            pixels.push((packed & 0xFF) as u8);
            pixels.push(((packed >> 8) & 0xFF) as u8);
            pixels.push(((packed >> 16) & 0xFF) as u8);
            pixels.push(((packed >> 24) & 0xFF) as u8);
        }
        drop(view);
        staging_buffer.unmap();

        let rgba = if upscale {
            upscale_bilinear(
                &pixels,
                internal_width,
                internal_height,
                config.width,
                config.height,
            )
        } else {
            pixels
        };

        Ok(RenderOutput {
            width: if upscale {
                config.width
            } else {
                internal_width
            },
            height: if upscale {
                config.height
            } else {
                internal_height
            },
            internal_width,
            internal_height,
            rgba,
            elapsed: started.elapsed(),
        })
    }
}

fn validate_size(config: &RenderConfig) -> Result<(), GpuError> {
    if config.width == 0 || config.height == 0 || config.width > 8192 || config.height > 8192 {
        return Err(GpuError::InvalidRender(format!(
            "invalid render size {}x{}",
            config.width, config.height
        )));
    }
    if !config.resolution_scale.is_finite()
        || config.resolution_scale <= 0.0
        || config.resolution_scale > 1.0
    {
        return Err(GpuError::InvalidRender(format!(
            "resolution_scale must be in (0, 1], got {}",
            config.resolution_scale
        )));
    }
    Ok(())
}

fn validate_camera(camera: &Camera) -> Result<(), GpuError> {
    if !camera.position.is_finite() || !camera.target.is_finite() || !camera.up.is_finite() {
        return Err(GpuError::InvalidRender(
            "camera contains non-finite values".to_string(),
        ));
    }
    if !camera.fov_y_degrees.is_finite()
        || camera.fov_y_degrees <= 1.0
        || camera.fov_y_degrees >= 179.0
    {
        return Err(GpuError::InvalidRender(format!(
            "fov_y_degrees must be in (1, 179), got {}",
            camera.fov_y_degrees
        )));
    }
    Ok(())
}

fn upscale_bilinear(
    source: &[u8],
    source_width: u32,
    source_height: u32,
    width: u32,
    height: u32,
) -> Vec<u8> {
    if source_width == width && source_height == height {
        return source.to_vec();
    }

    let mut output = vec![0u8; width as usize * height as usize * 4];
    let sw = source_width as f32;
    let sh = source_height as f32;

    for y in 0..height {
        let sy = ((y as f32 + 0.5) * sh / height as f32 - 0.5).clamp(0.0, sh - 1.0);
        let y0 = sy.floor() as u32;
        let y1 = (y0 + 1).min(source_height - 1);
        let fy = sy - y0 as f32;

        for x in 0..width {
            let sx = ((x as f32 + 0.5) * sw / width as f32 - 0.5).clamp(0.0, sw - 1.0);
            let x0 = sx.floor() as u32;
            let x1 = (x0 + 1).min(source_width - 1);
            let fx = sx - x0 as f32;

            let index = |px: u32, py: u32| -> usize { ((py * source_width + px) * 4) as usize };
            let p00 = index(x0, y0);
            let p10 = index(x1, y0);
            let p01 = index(x0, y1);
            let p11 = index(x1, y1);
            let out = ((y * width + x) * 4) as usize;

            for channel in 0..4 {
                let top =
                    source[p00 + channel] as f32 * (1.0 - fx) + source[p10 + channel] as f32 * fx;
                let bottom =
                    source[p01 + channel] as f32 * (1.0 - fx) + source[p11 + channel] as f32 * fx;
                output[out + channel] =
                    (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeroset_core::{FieldGraph, Vec3};

    fn sphere() -> FieldGraph {
        let mut graph = FieldGraph::new();
        let root = graph.sphere(Vec3::ZERO, 1.0);
        graph.set_root(root);
        graph
    }

    #[test]
    fn raymarch_sphere_produces_image_and_dynamic_resolution() {
        let renderer = match GpuRaymarchRenderer::new() {
            Ok(renderer) => renderer,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };

        let graph = sphere();
        let camera = Camera::looking_at(Vec3::new(3.0, 2.0, 3.0), Vec3::ZERO);
        let config = RenderConfig {
            width: 96,
            height: 64,
            resolution_scale: 0.5,
            max_steps: 64,
            step_scale: 0.9,
            epsilon: 0.0,
        };

        let output = renderer.render(&graph, &camera, &config).unwrap();
        assert_eq!(output.width, 96);
        assert_eq!(output.height, 64);
        assert_eq!(output.internal_width, 48);
        assert_eq!(output.internal_height, 32);
        assert_eq!(output.rgba.len(), 96 * 64 * 4);
        assert!(output.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255));

        let center = ((32 * 96 + 48) * 4) as usize;
        assert_ne!(
            &output.rgba[center..center + 4],
            &output.rgba[0..4],
            "center pixel should differ from background"
        );
    }

    #[test]
    fn progressive_render_keeps_last_pass() {
        let renderer = match GpuRaymarchRenderer::new() {
            Ok(renderer) => renderer,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };

        let graph = sphere();
        let camera = Camera::looking_at(Vec3::new(2.5, 1.5, 2.5), Vec3::ZERO);
        let config = RenderConfig {
            width: 64,
            height: 48,
            resolution_scale: 1.0,
            max_steps: 64,
            step_scale: 0.9,
            epsilon: 0.0,
        };
        let progressive = renderer
            .render_progressive(&graph, &camera, &config, &[0.25, 0.5, 1.0])
            .unwrap();

        assert_eq!(progressive.passes.len(), 3);
        assert_eq!(progressive.image.internal_width, 64);
        assert_eq!(progressive.image.internal_height, 48);
        assert_eq!(progressive.image.rgba.len(), 64 * 48 * 4);
    }
}
