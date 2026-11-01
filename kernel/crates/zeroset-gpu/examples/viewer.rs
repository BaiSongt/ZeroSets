use std::error::Error;
use std::time::{Duration, Instant};

use glam::Vec3;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowBuilder};

use zeroset_core::presets;
use zeroset_gpu::{Camera, compile_raymarch_texture_shader, compile_taa_shader};

const BLIT_WGSL: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var<uniform> target_size: vec4<f32>;

struct VsOut {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vs(@builtin(vertex_index) index: u32) -> VsOut {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0)
    );
    var out: VsOut;
    out.position = vec4<f32>(positions[index], 0.0, 1.0);
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let source_size = vec2<f32>(textureDimensions(src));
    let uv = in.position.xy / target_size.xy;
    let coordinate = vec2<i32>(
        clamp(uv * source_size, vec2<f32>(0.0), source_size - vec2<f32>(1.0))
    );
    return textureLoad(src, coordinate, 0);
}
"#;

const TILE_WGSL: &str = r#"
struct TileParams {
    size: vec4<f32>,
};

@group(0) @binding(0) var<uniform> tile_params: TileParams;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var<storage, read_write> quality: array<f32>;

@compute @workgroup_size(1, 1)
fn zs_tiles(@builtin(global_invocation_id) gid: vec3<u32>) {
    let width = u32(tile_params.size.x);
    let height = u32(tile_params.size.y);
    let tiles_x = (width + 15u) / 16u;
    let tiles_y = (height + 15u) / 16u;
    if (gid.x >= tiles_x || gid.y >= tiles_y) {
        return;
    }
    let index = gid.y * tiles_x + gid.x;
    if (index >= arrayLength(&quality)) {
        return;
    }

    let x0 = gid.x * 16u;
    let y0 = gid.y * 16u;
    let x1 = min(x0 + 16u, width);
    let y1 = min(y0 + 16u, height);

    var minimum = 1e20;
    var maximum = -1e20;
    var sum = 0.0;
    var sum_sq = 0.0;
    var count = 0.0;

    for (var y = y0; y < y1; y = y + 1u) {
        for (var x = x0; x < x1; x = x + 1u) {
            let color = textureLoad(src, vec2<i32>(i32(x), i32(y)), 0).rgb;
            let luminance = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
            minimum = min(minimum, luminance);
            maximum = max(maximum, luminance);
            sum = sum + luminance;
            sum_sq = sum_sq + luminance * luminance;
            count = count + 1.0;
        }
    }

    let mean = sum / max(count, 1.0);
    let variance = max(sum_sq / max(count, 1.0) - mean * mean, 0.0);
    let contrast = maximum - minimum;
    quality[index] = clamp(contrast * 2.0 + variance * 8.0, 0.0, 1.0);
}
"#;

fn main() -> Result<(), Box<dyn Error>> {
    let max_frames = parse_frames();
    let event_loop = EventLoop::new()?;
    let window = WindowBuilder::new()
        .with_title("ZeroSet · GPU raymarch")
        .with_inner_size(PhysicalSize::new(1280u32, 800u32))
        .build(&event_loop)?;
    let window: &'static Window = Box::leak(Box::new(window));

    let mut viewer = Viewer::new(window)?;
    let mut frames = 0u32;

    event_loop.run(move |event, target| {
        target.set_control_flow(ControlFlow::Wait);
        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => target.exit(),
                WindowEvent::Resized(size) => {
                    viewer.resize(size);
                    window.request_redraw();
                }
                WindowEvent::RedrawRequested => {
                    if let Err(error) = viewer.render() {
                        eprintln!("render error: {error}");
                        target.exit();
                        return;
                    }
                    frames += 1;
                    if let Some(limit) = max_frames {
                        if frames >= limit {
                            target.exit();
                        } else {
                            viewer.dirty = true;
                            window.request_redraw();
                        }
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    viewer.cursor_moved(position.x as f32, position.y as f32);
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    viewer.mouse_button(button, state);
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let amount = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(position) => position.y as f32 / 48.0,
                    };
                    viewer.zoom(amount);
                }
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed
                        && event.physical_key == PhysicalKey::Code(KeyCode::Escape) =>
                {
                    target.exit();
                }
                _ => {}
            },
            Event::AboutToWait => viewer.about_to_wait(target),
            _ => {}
        }
    })?;

    Ok(())
}

#[derive(Clone, Copy)]
struct PrevView {
    camera: Vec3,
    forward: Vec3,
    right: Vec3,
    up: Vec3,
    width: u32,
    height: u32,
    tan_half_fov: f32,
}

struct Viewer {
    window: &'static Window,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    raymarch_pipeline: wgpu::ComputePipeline,
    blit_pipeline: wgpu::RenderPipeline,
    tile_pipeline: wgpu::ComputePipeline,
    taa_pipeline: wgpu::ComputePipeline,
    raymarch_uniforms: wgpu::Buffer,
    blit_uniforms: wgpu::Buffer,
    tile_uniforms: wgpu::Buffer,
    taa_uniforms: wgpu::Buffer,
    taa_sampler: wgpu::Sampler,
    target: Option<RenderTarget>,
    internal_size: (u32, u32),
    reference: Vec3,
    bounds_scale: f32,
    target_point: Vec3,
    radius: f32,
    yaw: f32,
    pitch: f32,
    distance: f32,
    dragging: bool,
    last_mouse: Option<(f32, f32)>,
    last_interaction: Instant,
    dirty: bool,
    taa_samples: u32,
    taa_target: u32,
    accum_index: usize,
    prev_view: Option<PrevView>,
}

struct RenderTarget {
    _textures: [wgpu::Texture; 3],
    raymarch_bind_group: wgpu::BindGroup,
    analyze_bind_groups: [wgpu::BindGroup; 2],
    taa_bind_groups: [wgpu::BindGroup; 2],
    blit_bind_groups: [wgpu::BindGroup; 2],
}

impl Viewer {
    fn new(window: &'static Window) -> Result<Self, Box<dyn Error>> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window)?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            ..Default::default()
        }))?;
        let info = adapter.get_info();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("zeroset-viewer"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            }))?;

        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);
        let caps = surface.get_capabilities(&adapter);
        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| "surface has no default configuration".to_string())?;
        if let Some(format) = caps
            .formats
            .iter()
            .copied()
            .find(|format| !format.is_srgb())
        {
            config.format = format;
        }
        config.width = width;
        config.height = height;
        surface.configure(&device, &config);

        let storage_features =
            adapter.get_texture_format_features(wgpu::TextureFormat::Rgba16Float);
        if !storage_features
            .allowed_usages
            .contains(wgpu::TextureUsages::STORAGE_BINDING)
        {
            return Err("rgba16float does not support storage textures on this adapter".into());
        }

        let graph = presets::bracket();
        let bounds = graph.bounds();
        let reference = bounds.center();
        let local_graph = graph.rebased(reference);
        let bounds_scale = bounds.max_extent().max(1.0);
        let radius = (bounds.size().length() * 0.5).max(1.0);

        let raymarch_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("zeroset_raymarch_texture"),
            source: wgpu::ShaderSource::Wgsl(compile_raymarch_texture_shader(&local_graph)?.into()),
        });
        let raymarch_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("zeroset_raymarch_texture_pipeline"),
            layout: None,
            module: &raymarch_shader,
            entry_point: Some("zs_render"),
            compilation_options: Default::default(),
            cache: None,
        });

        let taa_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("zeroset_taa"),
            source: wgpu::ShaderSource::Wgsl(compile_taa_shader().into()),
        });
        let taa_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("zeroset_taa_pipeline"),
            layout: None,
            module: &taa_shader,
            entry_point: Some("zs_taa"),
            compilation_options: Default::default(),
            cache: None,
        });

        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("zeroset_blit"),
            source: wgpu::ShaderSource::Wgsl(BLIT_WGSL.into()),
        });
        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("zeroset_blit_pipeline"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &blit_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let tile_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("zeroset_tile_analyzer"),
            source: wgpu::ShaderSource::Wgsl(TILE_WGSL.into()),
        });
        let tile_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("zeroset_tile_pipeline"),
            layout: None,
            module: &tile_shader,
            entry_point: Some("zs_tiles"),
            compilation_options: Default::default(),
            cache: None,
        });

        let raymarch_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zeroset_raymarch_uniforms"),
            size: 112,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let blit_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zeroset_blit_uniforms"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tile_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zeroset_tile_uniforms"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let taa_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zeroset_taa_uniforms"),
            size: 96,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let taa_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("zeroset_taa_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        eprintln!("adapter: {} ({:?})", info.name, info.backend);

        let mut viewer = Self {
            window,
            surface,
            device,
            queue,
            config,
            raymarch_pipeline,
            blit_pipeline,
            tile_pipeline,
            taa_pipeline,
            raymarch_uniforms,
            blit_uniforms,
            tile_uniforms,
            taa_uniforms,
            taa_sampler,
            target: None,
            internal_size: (0, 0),
            reference,
            bounds_scale,
            target_point: reference,
            radius,
            yaw: 0.7,
            pitch: 0.35,
            distance: radius * 2.8,
            dragging: false,
            last_mouse: None,
            last_interaction: Instant::now() - Duration::from_secs(1),
            dirty: true,
            taa_samples: 0,
            taa_target: 16,
            accum_index: 0,
            prev_view: None,
        };
        viewer.ensure_target(width, height)?;
        Ok(viewer)
    }

    fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        let width = size.width.max(1);
        let height = size.height.max(1);
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.target = None;
        self.internal_size = (0, 0);
        self.dirty = true;
        self.taa_samples = 0;
        self.accum_index = 0;
        self.prev_view = None;
        self.last_interaction = Instant::now() - Duration::from_secs(1);
    }

    fn cursor_moved(&mut self, x: f32, y: f32) {
        if self.dragging {
            if let Some((previous_x, previous_y)) = self.last_mouse {
                self.yaw -= (x - previous_x) * 0.008;
                self.pitch = (self.pitch + (y - previous_y) * 0.008).clamp(-1.4, 1.4);
                self.mark_interaction();
            }
        }
        self.last_mouse = Some((x, y));
    }

    fn mouse_button(&mut self, button: MouseButton, state: ElementState) {
        if button == MouseButton::Left {
            self.dragging = state == ElementState::Pressed;
            if !self.dragging {
                self.last_mouse = None;
            }
            self.mark_interaction();
        }
    }

    fn zoom(&mut self, amount: f32) {
        if amount.abs() <= f32::EPSILON {
            return;
        }
        self.distance =
            (self.distance * (-amount * 0.12).exp()).clamp(self.radius * 0.4, self.radius * 20.0);
        self.mark_interaction();
    }

    fn mark_interaction(&mut self) {
        self.last_interaction = Instant::now();
        self.dirty = true;
        self.taa_samples = 0;
        self.window.request_redraw();
    }

    fn about_to_wait(&mut self, target: &EventLoopWindowTarget<()>) {
        if self.dirty {
            self.window.request_redraw();
            return;
        }
        let idle = Duration::from_millis(180);
        if self.last_interaction.elapsed() < idle {
            target.set_control_flow(ControlFlow::WaitUntil(self.last_interaction + idle));
            return;
        }
        if self.taa_samples < self.taa_target {
            self.window.request_redraw();
        }
    }

    fn camera(&self) -> Camera {
        let direction = Vec3::new(
            self.pitch.cos() * self.yaw.sin(),
            self.pitch.sin(),
            self.pitch.cos() * self.yaw.cos(),
        );
        Camera::looking_at(
            self.target_point + direction * self.distance,
            self.target_point,
        )
        .with_fov(42.0)
    }

    fn ensure_target(&mut self, width: u32, height: u32) -> Result<(), Box<dyn Error>> {
        let quick = self.dirty && self.last_interaction.elapsed() < Duration::from_millis(180);
        let scale = if quick { 0.5 } else { 1.0 };
        let internal_width = ((width as f32 * scale).round() as u32).max(1);
        let internal_height = ((height as f32 * scale).round() as u32).max(1);

        if self.target.is_some() && self.internal_size == (internal_width, internal_height) {
            return Ok(());
        }

        let create_texture = |label: &str| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: internal_width,
                    height: internal_height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };

        let current = create_texture("zeroset_current");
        let accum_a = create_texture("zeroset_accum_a");
        let accum_b = create_texture("zeroset_accum_b");
        let current_view = current.create_view(&wgpu::TextureViewDescriptor::default());
        let accum_views = [
            accum_a.create_view(&wgpu::TextureViewDescriptor::default()),
            accum_b.create_view(&wgpu::TextureViewDescriptor::default()),
        ];

        let tiles_x = internal_width.div_ceil(16);
        let tiles_y = internal_height.div_ceil(16);
        let tile_count = (tiles_x * tiles_y) as usize;
        let quality_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zeroset_tile_quality"),
            size: (tile_count * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ones = vec![1.0f32; tile_count];
        self.queue
            .write_buffer(&quality_buffer, 0, bytemuck::cast_slice(&ones));
        let tile_size = [internal_width as f32, internal_height as f32, 0.0, 0.0];
        self.queue
            .write_buffer(&self.tile_uniforms, 0, bytemuck::cast_slice(&tile_size));

        let raymarch_layout = self.raymarch_pipeline.get_bind_group_layout(0);
        let raymarch_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("zeroset_raymarch_bind_group"),
            layout: &raymarch_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.raymarch_uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&current_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: quality_buffer.as_entire_binding(),
                },
            ],
        });

        let tile_layout = self.tile_pipeline.get_bind_group_layout(0);
        let analyze_bind_groups = [
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("zeroset_tile_bind_group_a"),
                layout: &tile_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.tile_uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&accum_views[0]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: quality_buffer.as_entire_binding(),
                    },
                ],
            }),
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("zeroset_tile_bind_group_b"),
                layout: &tile_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.tile_uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&accum_views[1]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: quality_buffer.as_entire_binding(),
                    },
                ],
            }),
        ];

        let taa_layout = self.taa_pipeline.get_bind_group_layout(0);
        let taa_bind_groups = [
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("zeroset_taa_bind_group_a"),
                layout: &taa_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.raymarch_uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&current_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&accum_views[1]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&accum_views[0]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: self.taa_uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::Sampler(&self.taa_sampler),
                    },
                ],
            }),
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("zeroset_taa_bind_group_b"),
                layout: &taa_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.raymarch_uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&current_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&accum_views[0]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&accum_views[1]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: self.taa_uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::Sampler(&self.taa_sampler),
                    },
                ],
            }),
        ];

        let blit_layout = self.blit_pipeline.get_bind_group_layout(0);
        let blit_bind_groups = [
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("zeroset_blit_bind_group_a"),
                layout: &blit_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&accum_views[0]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.blit_uniforms.as_entire_binding(),
                    },
                ],
            }),
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("zeroset_blit_bind_group_b"),
                layout: &blit_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&accum_views[1]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.blit_uniforms.as_entire_binding(),
                    },
                ],
            }),
        ];

        self.target = Some(RenderTarget {
            _textures: [current, accum_a, accum_b],
            raymarch_bind_group,
            analyze_bind_groups,
            taa_bind_groups,
            blit_bind_groups,
        });
        self.internal_size = (internal_width, internal_height);
        self.taa_samples = 0;
        self.accum_index = 0;
        self.prev_view = None;
        Ok(())
    }

    fn render(&mut self) -> Result<(), Box<dyn Error>> {
        let started = Instant::now();
        let quick = self.last_interaction.elapsed() < Duration::from_millis(180);
        let idle = !quick;
        if !self.dirty && !(idle && self.taa_samples < self.taa_target) {
            return Ok(());
        }

        let width = self.config.width.max(1);
        let height = self.config.height.max(1);
        self.ensure_target(width, height)?;
        let (internal_width, internal_height) = self.internal_size;

        let camera = self.camera();
        let camera_position = camera.position - self.reference;
        let camera_target = camera.target - self.reference;
        let forward = (camera_target - camera_position).normalize_or_zero();
        let right = forward.cross(camera.up).normalize_or_zero();
        let up = right.cross(forward).normalize_or_zero();
        let tan_half_fov = (42.0f32.to_radians() * 0.5).tan();
        let pixel_angle = 2.0 * tan_half_fov / internal_height as f32;
        let epsilon = (self.bounds_scale * 1e-4).max(1e-5);
        let max_distance = self.bounds_scale * 8.0 + camera_position.length() + 1.0;
        let scale = internal_width as f32 / width as f32;
        let sample_index = if quick { 0 } else { self.taa_samples };
        let (jitter_x, jitter_y) = if quick {
            (0.0, 0.0)
        } else {
            (
                halton(sample_index + 1, 2) - 0.5,
                halton(sample_index + 1, 3) - 0.5,
            )
        };
        let history_alpha = if self.prev_view.is_none() {
            1.0
        } else if quick {
            0.2
        } else {
            1.0 / (sample_index as f32 + 1.0)
        };

        let mut uniforms = [0.0f32; 28];
        uniforms[0..3].copy_from_slice(&camera_position.to_array());
        uniforms[4..7].copy_from_slice(&forward.to_array());
        uniforms[8..11].copy_from_slice(&right.to_array());
        uniforms[12..15].copy_from_slice(&up.to_array());
        uniforms[16] = internal_width as f32;
        uniforms[17] = internal_height as f32;
        uniforms[18] = tan_half_fov;
        uniforms[19] = pixel_angle;
        uniforms[20] = if quick { 96.0 } else { 160.0 };
        uniforms[21] = epsilon;
        uniforms[22] = max_distance;
        uniforms[23] = 0.9;
        uniforms[24] = jitter_x;
        uniforms[25] = jitter_y;
        uniforms[26] = history_alpha;
        uniforms[27] = sample_index as f32;
        self.queue
            .write_buffer(&self.raymarch_uniforms, 0, bytemuck::cast_slice(&uniforms));

        let mut previous = [0.0f32; 24];
        let mut history_valid = false;
        if let Some(view) = self.prev_view {
            if view.width == internal_width && view.height == internal_height {
                previous[0..3].copy_from_slice(&view.camera.to_array());
                previous[4..7].copy_from_slice(&view.forward.to_array());
                previous[8..11].copy_from_slice(&view.right.to_array());
                previous[12..15].copy_from_slice(&view.up.to_array());
                previous[16] = view.width as f32;
                previous[17] = view.height as f32;
                previous[18] = view.tan_half_fov;
                previous[19] = 1.0;
                previous[20] = history_alpha;
                history_valid = true;
            }
        }
        if !history_valid {
            previous[20] = 1.0;
        }
        self.queue
            .write_buffer(&self.taa_uniforms, 0, bytemuck::cast_slice(&previous));

        let blit_size = [width as f32, height as f32, 0.0, 0.0];
        self.queue
            .write_buffer(&self.blit_uniforms, 0, bytemuck::cast_slice(&blit_size));

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("surface validation error".into());
            }
        };
        let frame_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let output_index = self.accum_index;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("zeroset_viewer_encoder"),
            });

        if history_valid {
            let target = self.target.as_ref().expect("target exists");
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("zeroset_tile_analysis"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.tile_pipeline);
            pass.set_bind_group(0, &target.analyze_bind_groups[1 - output_index], &[]);
            pass.dispatch_workgroups(internal_width.div_ceil(16), internal_height.div_ceil(16), 1);
        }
        {
            let target = self.target.as_ref().expect("target exists");
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("zeroset_raymarch_compute"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.raymarch_pipeline);
            pass.set_bind_group(0, &target.raymarch_bind_group, &[]);
            pass.dispatch_workgroups(internal_width.div_ceil(8), internal_height.div_ceil(8), 1);
        }
        {
            let target = self.target.as_ref().expect("target exists");
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("zeroset_taa_compute"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.taa_pipeline);
            pass.set_bind_group(0, &target.taa_bind_groups[output_index], &[]);
            pass.dispatch_workgroups(internal_width.div_ceil(8), internal_height.div_ceil(8), 1);
        }
        {
            let target = self.target.as_ref().expect("target exists");
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("zeroset_blit_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &target.blit_bind_groups[output_index], &[]);
            pass.draw(0..6, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);

        self.prev_view = Some(PrevView {
            camera: camera_position,
            forward,
            right,
            up,
            width: internal_width,
            height: internal_height,
            tan_half_fov,
        });
        if quick {
            self.taa_samples = 0;
        } else {
            self.taa_samples = (self.taa_samples + 1).min(self.taa_target);
        }
        self.accum_index = 1 - self.accum_index;
        self.dirty = false;

        self.window.set_title(&format!(
            "ZeroSet · GPU raymarch · {:.2}x · TAA {}/{} · {:.2} ms",
            scale,
            self.taa_samples,
            self.taa_target,
            started.elapsed().as_secs_f64() * 1000.0
        ));

        Ok(())
    }
}

fn halton(mut index: u32, base: u32) -> f32 {
    let mut result = 0.0;
    let mut fraction = 1.0;
    while index > 0 {
        fraction /= base as f32;
        result += fraction * (index % base) as f32;
        index /= base;
    }
    result
}

fn parse_frames() -> Option<u32> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--frames" {
            return args.next().and_then(|value| value.parse().ok());
        }
    }
    None
}
