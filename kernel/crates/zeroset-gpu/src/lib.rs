//! GPU compute backend for the ZeroSet field DAG.
//!
//! The IR is compiled to WGSL on the fly. The core capability is batched field
//! evaluation; on top of that the crate exposes tiled sampling and an adaptive
//! octree sampler used by the viewport and high-resolution meshing paths.

mod adaptive;
mod adaptive_f64;
mod adaptive_mesh;
mod adaptive_mixed;
mod raymarch;
mod wgsl;

pub use adaptive::{AdaptiveConfig, AdaptiveGrid, sample_adaptive};
pub use adaptive_f64::{
    mesh_adaptive_f64, mesh_adaptive_f64_dc, mesh_adaptive_refined_dc_f64,
    mesh_adaptive_refined_f64,
};
pub use adaptive_mesh::{
    mesh_adaptive, mesh_adaptive_dc, mesh_adaptive_refined, mesh_adaptive_refined_dc,
    refine_adaptive_grid,
};
pub use adaptive_mixed::{mesh_adaptive_mixed, mesh_adaptive_mixed_dc};
pub use raymarch::{
    Camera, GpuRaymarchRenderer, ProgressiveRender, RenderConfig, RenderOutput, RenderPass,
};
pub use wgsl::{
    compile_field_functions, compile_raymarch_shader, compile_raymarch_texture_shader,
    compile_shader, compile_taa_shader,
};

use zeroset_core::FieldGraph;

#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("field graph has no root node")]
    EmptyGraph,

    #[error("field graph references invalid node id {0}")]
    InvalidNode(u32),

    #[error("node {0} contains a non-finite parameter")]
    NonFinite(u32),

    #[error("no suitable GPU adapter: {0}")]
    NoAdapter(String),

    #[error("failed to create GPU device: {0}")]
    Device(String),

    #[error("GPU buffer mapping failed: {0}")]
    Mapping(String),

    #[error("GPU poll failed: {0}")]
    Poll(String),

    #[error("invalid tile: {0}")]
    InvalidTile(String),

    #[error("too many points in one dispatch: {0}")]
    TooManyPoints(usize),

    #[error("invalid adaptive sampling configuration: {0}")]
    InvalidAdaptive(String),

    #[error("invalid render configuration: {0}")]
    InvalidRender(String),
}

/// A regular grid tile evaluated in one batch.
///
/// `dims` counts corner samples per axis (cells + 1), matching
/// [`zeroset_core::GridSamples`]. Positions are `origin + (x, y, z) * cell`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileSpec {
    pub origin: [f32; 3],
    pub cell: f32,
    pub dims: [u32; 3],
}

impl TileSpec {
    pub fn point_count(&self) -> usize {
        self.dims[0] as usize * self.dims[1] as usize * self.dims[2] as usize
    }

    pub fn position(&self, x: u32, y: u32, z: u32) -> [f32; 3] {
        [
            self.origin[0] + x as f32 * self.cell,
            self.origin[1] + y as f32 * self.cell,
            self.origin[2] + z as f32 * self.cell,
        ]
    }

    fn validate(&self) -> Result<(), GpuError> {
        if self.dims.iter().any(|dim| *dim < 2) {
            return Err(GpuError::InvalidTile(format!(
                "dims {:?} must each be at least 2",
                self.dims
            )));
        }
        if !self.cell.is_finite() || self.cell <= 0.0 {
            return Err(GpuError::InvalidTile(format!(
                "cell size {} must be finite and positive",
                self.cell
            )));
        }
        if self.origin.iter().any(|value| !value.is_finite()) {
            return Err(GpuError::InvalidTile(
                "origin contains a non-finite component".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct TileValues {
    pub spec: TileSpec,
    pub values: Vec<f32>,
}

/// Headless wgpu compute context for field evaluation.
pub struct GpuSampler {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_name: String,
    backend: String,
}

impl GpuSampler {
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
            label: Some("zeroset-gpu"),
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

    /// Evaluate `graph` at every point. The returned vector has one value per
    /// input point, in the same order.
    pub fn sample(&self, graph: &FieldGraph, points: &[[f32; 3]]) -> Result<Vec<f32>, GpuError> {
        if points.is_empty() {
            return Ok(Vec::new());
        }
        let pipeline = self.compile_pipeline(graph)?;
        self.dispatch(&pipeline, points)
    }

    /// Evaluate a batch of grid tiles with one shader compilation and one
    /// dispatch. This is the high-throughput path used by adaptive sampling.
    pub fn sample_tiles(
        &self,
        graph: &FieldGraph,
        tiles: &[TileSpec],
    ) -> Result<Vec<TileValues>, GpuError> {
        if tiles.is_empty() {
            return Ok(Vec::new());
        }

        let pipeline = self.compile_pipeline(graph)?;
        let mut counts = Vec::with_capacity(tiles.len());
        let mut points: Vec<[f32; 3]> = Vec::new();

        for tile in tiles {
            tile.validate()?;
            let count = tile.point_count();
            counts.push(count);
            points.reserve(count);
            for z in 0..tile.dims[2] {
                for y in 0..tile.dims[1] {
                    for x in 0..tile.dims[0] {
                        points.push(tile.position(x, y, z));
                    }
                }
            }
        }

        let all_values = self.dispatch(&pipeline, &points)?;
        let mut result = Vec::with_capacity(tiles.len());
        let mut offset = 0;
        for (tile, count) in tiles.iter().zip(counts) {
            result.push(TileValues {
                spec: *tile,
                values: all_values[offset..offset + count].to_vec(),
            });
            offset += count;
        }
        Ok(result)
    }

    fn compile_pipeline(&self, graph: &FieldGraph) -> Result<wgpu::ComputePipeline, GpuError> {
        let shader_source = compile_shader(graph)?;
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("zeroset_field"),
                source: wgpu::ShaderSource::Wgsl(shader_source.into()),
            });

        Ok(self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("zeroset_sample"),
                layout: None,
                module: &shader,
                entry_point: Some("zs_main"),
                compilation_options: Default::default(),
                cache: None,
            }))
    }

    fn dispatch(
        &self,
        pipeline: &wgpu::ComputePipeline,
        points: &[[f32; 3]],
    ) -> Result<Vec<f32>, GpuError> {
        // wgpu limits one dispatch to 65535 workgroups; with 64 invocations per
        // group that is 4,194,240 points. Stay comfortably below the limit and
        // let a high-resolution adaptive pass span multiple dispatches.
        const MAX_POINTS_PER_DISPATCH: usize = 2_000_000;

        let mut values = Vec::with_capacity(points.len());
        for chunk in points.chunks(MAX_POINTS_PER_DISPATCH) {
            values.extend(self.dispatch_chunk(pipeline, chunk)?);
        }
        Ok(values)
    }

    fn dispatch_chunk(
        &self,
        pipeline: &wgpu::ComputePipeline,
        points: &[[f32; 3]],
    ) -> Result<Vec<f32>, GpuError> {
        if points.is_empty() {
            return Ok(Vec::new());
        }
        if points.len() > u32::MAX as usize {
            return Err(GpuError::TooManyPoints(points.len()));
        }

        let layout = pipeline.get_bind_group_layout(0);
        let count = points.len();
        let count_u32 = count as u32;
        let byte_len = (count * 4) as u64;

        let mut flat_positions = Vec::with_capacity(count * 3);
        for point in points {
            flat_positions.extend_from_slice(point);
        }

        let positions_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zeroset_positions"),
            size: (flat_positions.len() * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue
            .write_buffer(&positions_buffer, 0, bytemuck::cast_slice(&flat_positions));

        let values_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zeroset_values"),
            size: byte_len,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let staging_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zeroset_staging"),
            size: byte_len,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let params: [u32; 4] = [count_u32, 0, 0, 0];
        let params_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zeroset_params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue
            .write_buffer(&params_buffer, 0, bytemuck::cast_slice(&params));

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("zeroset_bind_group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: positions_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: values_buffer.as_entire_binding(),
                },
            ],
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("zeroset_encoder"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("zeroset_sample_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(count_u32.div_ceil(64), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&values_buffer, 0, &staging_buffer, 0, byte_len);
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
        let mut values = Vec::with_capacity(count);
        for chunk in view.chunks_exact(4) {
            values.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        drop(view);
        staging_buffer.unmap();

        Ok(values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeroset_core::Vec3;

    #[test]
    fn gpu_sphere_matches_cpu() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };

        let mut graph = FieldGraph::new();
        let root = graph.sphere(Vec3::new(0.25, -0.5, 0.75), 1.0);
        graph.set_root(root);

        let points: Vec<[f32; 3]> = (0..32)
            .map(|i| {
                let t = i as f32 * 0.1;
                [t.sin(), t.cos(), t * 0.25]
            })
            .collect();

        let gpu = sampler.sample(&graph, &points).unwrap();
        assert_eq!(gpu.len(), points.len());

        for (point, gpu_value) in points.iter().zip(gpu) {
            let cpu = graph.evaluate(Vec3::from_array(*point));
            assert!(
                (gpu_value - cpu).abs() < 1e-3,
                "gpu={gpu_value} cpu={cpu} at {point:?}"
            );
        }

        eprintln!(
            "gpu adapter: {} ({})",
            sampler.adapter_name(),
            sampler.backend()
        );
    }

    #[test]
    fn tiled_sampling_matches_direct_sampling() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };

        let mut graph = FieldGraph::new();
        let root = graph.sphere(Vec3::ZERO, 1.0);
        graph.set_root(root);

        let tile = TileSpec {
            origin: [-1.0, -1.0, -1.0],
            cell: 0.25,
            dims: [9, 9, 9],
        };
        let tiles = sampler.sample_tiles(&graph, &[tile]).unwrap();
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].values.len(), tile.point_count());

        for z in 0..tile.dims[2] {
            for y in 0..tile.dims[1] {
                for x in 0..tile.dims[0] {
                    let index = (x + tile.dims[0] * (y + tile.dims[1] * z)) as usize;
                    let point = Vec3::from_array(tile.position(x, y, z));
                    let cpu = graph.evaluate(point);
                    let gpu = tiles[0].values[index];
                    assert!((gpu - cpu).abs() < 1e-3, "gpu={gpu} cpu={cpu} at {point:?}");
                }
            }
        }
    }
}
