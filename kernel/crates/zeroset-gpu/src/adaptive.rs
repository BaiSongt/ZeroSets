use glam::Vec3;

use zeroset_core::{Aabb, FieldGraph};

use crate::{GpuError, GpuSampler, TileSpec, TileValues};

/// Configuration for dynamic, adaptive-resolution sampling.
///
/// The sampler starts with a cube of `base_cells` cells around `bounds` and
/// recursively subdivides only tiles that are near or cross the zero set. Each
/// tile keeps a fixed `base_cells + 1` corner grid, so resolution doubles per
/// level while memory grows with surface area, not volume.
#[derive(Clone, Copy, Debug)]
pub struct AdaptiveConfig {
    pub bounds: Aabb,
    pub base_cells: u32,
    pub max_depth: u32,
    /// A tile is kept when any corner is within `band_cells * tile.cell` of the
    /// surface, or when the tile has a sign change.
    pub band_cells: f32,
    pub max_tiles: usize,
}

impl AdaptiveConfig {
    pub fn new(bounds: Aabb) -> Self {
        Self {
            bounds,
            base_cells: 16,
            max_depth: 4,
            band_cells: 2.0,
            max_tiles: 4096,
        }
    }
}

/// Result of adaptive sampling. Tile origins are in the local frame defined by
/// `reference`; add `reference` to get world-space positions.
#[derive(Clone, Debug)]
pub struct AdaptiveGrid {
    pub reference: Vec3,
    pub tiles: Vec<TileValues>,
    /// Number of active tiles kept per level; `levels[0]` is the root level.
    pub levels: Vec<usize>,
    pub truncated: bool,
}

impl AdaptiveGrid {
    pub fn point_count(&self) -> usize {
        self.tiles.iter().map(|tile| tile.values.len()).sum()
    }

    pub fn finest_cell(&self) -> Option<f32> {
        self.tiles
            .iter()
            .map(|tile| tile.spec.cell)
            .reduce(f32::min)
    }
}

/// GPU adaptive sampling.
///
/// The field graph is rebased to `config.bounds.center()` before evaluation, so
/// local tile coordinates stay small even when the model lives far from the
/// world origin. World-space consumers add `AdaptiveGrid::reference` back.
pub fn sample_adaptive(
    sampler: &GpuSampler,
    graph: &FieldGraph,
    config: &AdaptiveConfig,
) -> Result<AdaptiveGrid, GpuError> {
    validate(config)?;

    let reference = config.bounds.center();
    let local_graph = graph.rebased(reference);
    let extent = config.bounds.max_extent();
    let base = config.base_cells;
    let cell = extent / base as f32;

    // Root is a cube centered on the bounds; the model fits inside it.
    let half = 0.5 * base as f32 * cell;
    let root = TileSpec {
        origin: [-half, -half, -half],
        cell,
        dims: [base + 1; 3],
    };

    let mut pending = vec![root];
    let mut leaves: Vec<TileValues> = Vec::new();
    let mut levels = Vec::new();
    let mut truncated = false;

    for depth in 0..=config.max_depth {
        if pending.is_empty() {
            break;
        }

        let evaluated = sampler.sample_tiles(&local_graph, &pending)?;
        let band_scale = config.band_cells.max(1.0);
        let mut active_tiles = Vec::new();

        for tile in evaluated {
            let band = band_scale * tile.spec.cell;
            if !tile_is_active(&tile.values, band) {
                continue;
            }
            active_tiles.push(tile);
        }

        levels.push(active_tiles.len());

        if depth == config.max_depth {
            leaves.extend(active_tiles);
            break;
        }

        let mut next = Vec::new();
        for tile in active_tiles {
            if leaves.len() + next.len() + 8 > config.max_tiles {
                truncated = true;
                leaves.push(tile);
                continue;
            }
            subdivide(&tile.spec, &mut next);
        }
        pending = next;
    }

    Ok(AdaptiveGrid {
        reference,
        tiles: leaves,
        levels,
        truncated,
    })
}

fn validate(config: &AdaptiveConfig) -> Result<(), GpuError> {
    if config.bounds.is_empty() || !config.bounds.is_finite() {
        return Err(GpuError::InvalidAdaptive(
            "bounds must be finite and non-empty".to_string(),
        ));
    }
    if config.bounds.max_extent() <= 0.0 {
        return Err(GpuError::InvalidAdaptive(
            "bounds must have a positive extent".to_string(),
        ));
    }
    if config.base_cells < 2 {
        return Err(GpuError::InvalidAdaptive(
            "base_cells must be at least 2".to_string(),
        ));
    }
    if config.max_depth > 16 {
        return Err(GpuError::InvalidAdaptive(
            "max_depth above 16 is not supported".to_string(),
        ));
    }
    if !config.band_cells.is_finite() || config.band_cells < 1.0 {
        return Err(GpuError::InvalidAdaptive(
            "band_cells must be at least 1".to_string(),
        ));
    }
    if config.max_tiles == 0 {
        return Err(GpuError::InvalidAdaptive(
            "max_tiles must be positive".to_string(),
        ));
    }
    Ok(())
}

fn tile_is_active(values: &[f32], band: f32) -> bool {
    let mut negative = false;
    let mut positive = false;
    let mut near = false;

    for value in values {
        if !value.is_finite() {
            // Never drop a tile that contains invalid samples; the caller can
            // surface a diagnostic instead of silently losing geometry.
            return true;
        }
        if *value < 0.0 {
            negative = true;
        } else {
            positive = true;
        }
        if value.abs() <= band {
            near = true;
        }
        if negative && positive {
            return true;
        }
    }

    near
}

fn subdivide(parent: &TileSpec, children: &mut Vec<TileSpec>) {
    let child_cell = parent.cell * 0.5;
    let offset = parent.cell * (parent.dims[0] - 1) as f32 * 0.5;
    for z in 0..2 {
        for y in 0..2 {
            for x in 0..2 {
                children.push(TileSpec {
                    origin: [
                        parent.origin[0] + x as f32 * offset,
                        parent.origin[1] + y as f32 * offset,
                        parent.origin[2] + z as f32 * offset,
                    ],
                    cell: child_cell,
                    dims: parent.dims,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeroset_core::{FieldGraph, Vec3};

    fn sphere_graph() -> FieldGraph {
        let mut graph = FieldGraph::new();
        let root = graph.sphere(Vec3::ZERO, 1.0);
        graph.set_root(root);
        graph
    }

    #[test]
    fn adaptive_sphere_keeps_only_surface_tiles() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };

        let graph = sphere_graph();
        let config = AdaptiveConfig {
            bounds: graph.bounds(),
            base_cells: 8,
            max_depth: 3,
            band_cells: 2.0,
            max_tiles: 4096,
        };
        let adaptive = sample_adaptive(&sampler, &graph, &config).unwrap();

        assert!(!adaptive.tiles.is_empty());
        assert_eq!(adaptive.levels.len(), 4);
        assert!(!adaptive.truncated);
        assert!(adaptive.point_count() > 0);
        assert!(adaptive.finest_cell().unwrap() < 0.25);

        let full_cells = 8u32 * 8;
        let full_points = (full_cells + 1).pow(3) as usize;
        assert!(adaptive.point_count() < full_points);

        for tile in &adaptive.tiles {
            assert!(tile.values.iter().all(|value| value.is_finite()));
        }
    }

    #[test]
    fn truncated_adaptive_grid_can_be_refined_then_meshed() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };

        let graph = sphere_graph();
        let config = AdaptiveConfig {
            bounds: graph.bounds(),
            base_cells: 8,
            max_depth: 2,
            band_cells: 2.0,
            max_tiles: 12,
        };
        let adaptive = sample_adaptive(&sampler, &graph, &config).unwrap();
        assert!(adaptive.truncated, "expected mixed-level leaves");
        assert!(crate::mesh_adaptive(&adaptive).is_err());

        let mesh = crate::mesh_adaptive_refined(&graph, &adaptive).unwrap();
        let stats = mesh.stats();
        assert!(stats.triangles > 0);
        assert_eq!(stats.boundary_edges, 0);
        assert!(stats.is_closed);
        assert!(stats.signed_volume > 0.0);
    }

    #[test]
    fn adaptive_dc_is_closed() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };
        let graph = sphere_graph();
        let config = AdaptiveConfig {
            bounds: graph.bounds(),
            base_cells: 8,
            max_depth: 2,
            band_cells: 2.0,
            max_tiles: 4096,
        };
        let adaptive = sample_adaptive(&sampler, &graph, &config).unwrap();
        let mesh = crate::mesh_adaptive_dc(&graph, &adaptive).unwrap();
        let stats = mesh.stats();
        assert!(stats.triangles > 0);
        assert_eq!(stats.boundary_edges, 0);
        assert!(stats.is_closed);
    }

    #[test]
    fn truncated_adaptive_grid_can_be_refined_then_dc_meshed() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };

        let graph = sphere_graph();
        let config = AdaptiveConfig {
            bounds: graph.bounds(),
            base_cells: 8,
            max_depth: 2,
            band_cells: 2.0,
            max_tiles: 12,
        };
        let adaptive = sample_adaptive(&sampler, &graph, &config).unwrap();
        assert!(adaptive.truncated);

        let mesh = crate::mesh_adaptive_refined_dc(&graph, &adaptive).unwrap();
        let stats = mesh.stats();
        assert!(stats.triangles > 0);
        assert_eq!(stats.boundary_edges, 0);
        assert!(stats.is_closed);
        assert!(stats.signed_volume > 0.0);
    }

    #[test]
    fn truncated_adaptive_grid_can_be_refined_then_f64_meshed() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };

        let graph = sphere_graph();
        let config = AdaptiveConfig {
            bounds: graph.bounds(),
            base_cells: 8,
            max_depth: 2,
            band_cells: 2.0,
            max_tiles: 12,
        };
        let adaptive = sample_adaptive(&sampler, &graph, &config).unwrap();
        assert!(adaptive.truncated);

        let mesh = crate::mesh_adaptive_refined_f64(&graph, &adaptive).unwrap();
        let stats = mesh.to_f32_world().stats();
        assert!(stats.triangles > 0);
        assert_eq!(stats.boundary_edges, 0);
        assert!(stats.is_closed);
        assert!(stats.signed_volume > 0.0);
    }

    #[test]
    fn truncated_adaptive_grid_can_be_refined_then_f64_dc_meshed() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };

        let graph = sphere_graph();
        let config = AdaptiveConfig {
            bounds: graph.bounds(),
            base_cells: 8,
            max_depth: 2,
            band_cells: 2.0,
            max_tiles: 12,
        };
        let adaptive = sample_adaptive(&sampler, &graph, &config).unwrap();
        assert!(adaptive.truncated);

        let mesh = crate::mesh_adaptive_refined_dc_f64(&graph, &adaptive).unwrap();
        let stats = mesh.to_f32_world().stats();
        assert!(stats.triangles > 0);
        assert_eq!(stats.boundary_edges, 0);
        assert!(stats.is_closed);
        assert!(stats.signed_volume > 0.0);
    }

    #[test]
    fn mixed_level_prototype_produces_mesh() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };
        let graph = sphere_graph();
        let config = AdaptiveConfig {
            bounds: graph.bounds(),
            base_cells: 8,
            max_depth: 2,
            band_cells: 2.0,
            max_tiles: 12,
        };
        let adaptive = sample_adaptive(&sampler, &graph, &config).unwrap();
        assert!(adaptive.truncated);
        let mesh = crate::mesh_adaptive_mixed(&graph, &adaptive).unwrap();
        let stats = mesh.stats();
        let refined = crate::mesh_adaptive_refined(&graph, &adaptive).unwrap();
        let dc = crate::mesh_adaptive_mixed_dc(&graph, &adaptive).unwrap();
        let dc_stats = dc.stats();
        let dc_hausdorff = refined.hausdorff_distance(&dc, 3);
        eprintln!(
            "mixed DC: verts={} tris={} closed={} boundary={} nonmanifold={} volume={} hausdorff={dc_hausdorff}",
            dc_stats.vertices,
            dc_stats.triangles,
            dc_stats.is_closed,
            dc_stats.boundary_edges,
            dc_stats.non_manifold_edges,
            dc_stats.signed_volume
        );
        assert!(dc_stats.is_closed);
        assert_eq!(dc_stats.boundary_edges, 0);
        assert!(dc_hausdorff < 0.02);
        assert!(dc_stats.signed_volume > 4.1);
        assert_eq!(dc_stats.vertices, stats.vertices);
        assert_eq!(dc_stats.triangles, stats.triangles);
        let hausdorff = refined.hausdorff_distance(&mesh, 3);
        eprintln!(
            "mixed prototype: verts={} tris={} closed={} boundary={} nonmanifold={} volume={} hausdorff={hausdorff}",
            stats.vertices,
            stats.triangles,
            stats.is_closed,
            stats.boundary_edges,
            stats.non_manifold_edges,
            stats.signed_volume
        );
        assert!(stats.triangles > 0);
        assert!(stats.is_closed);
        assert_eq!(stats.boundary_edges, 0);
        assert_eq!(stats.non_manifold_edges, 0);
        assert!(stats.signed_volume > 4.0);
        assert!(hausdorff < 0.02, "mixed Hausdorff {hausdorff}");
    }

    #[test]
    fn mixed_level_sharp_box_is_closed() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };
        let mut graph = FieldGraph::new();
        let node = graph.rounded_box(Vec3::ZERO, Vec3::splat(1.0), 0.0);
        graph.set_root(node);
        let config = AdaptiveConfig {
            bounds: graph.bounds(),
            base_cells: 8,
            max_depth: 2,
            band_cells: 2.0,
            max_tiles: 12,
        };
        let adaptive = sample_adaptive(&sampler, &graph, &config).unwrap();
        let mesh = crate::mesh_adaptive_mixed_dc(&graph, &adaptive).unwrap();
        let stats = mesh.stats();
        let refined = crate::mesh_adaptive_refined(&graph, &adaptive).unwrap();
        let hausdorff = refined.hausdorff_distance(&mesh, 3);
        eprintln!(
            "mixed sharp box: truncated={} verts={} tris={} closed={} boundary={} nonmanifold={} volume={} hausdorff={hausdorff}",
            adaptive.truncated,
            stats.vertices,
            stats.triangles,
            stats.is_closed,
            stats.boundary_edges,
            stats.non_manifold_edges,
            stats.signed_volume
        );
        assert!(stats.triangles > 0);
        assert!(stats.is_closed);
        assert_eq!(stats.boundary_edges, 0);
        assert!(stats.signed_volume > 7.9);
        assert!(hausdorff < 0.1, "mixed sharp box Hausdorff {hausdorff}");
    }

    #[test]
    fn mixed_level_thin_wall_is_closed() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };
        let mut graph = FieldGraph::new();
        let node = graph.rounded_box(Vec3::ZERO, Vec3::new(1.0, 1.0, 0.05), 0.0);
        graph.set_root(node);
        let config = AdaptiveConfig {
            bounds: graph.bounds(),
            base_cells: 16,
            max_depth: 2,
            band_cells: 2.0,
            max_tiles: 12,
        };
        let adaptive = sample_adaptive(&sampler, &graph, &config).unwrap();
        let mesh = crate::mesh_adaptive_mixed_dc(&graph, &adaptive).unwrap();
        let stats = mesh.stats();
        let refined = crate::mesh_adaptive_refined(&graph, &adaptive).unwrap();
        let hausdorff = refined.hausdorff_distance(&mesh, 3);
        eprintln!(
            "mixed thin wall: truncated={} verts={} tris={} closed={} boundary={} nonmanifold={} volume={} hausdorff={hausdorff}",
            adaptive.truncated,
            stats.vertices,
            stats.triangles,
            stats.is_closed,
            stats.boundary_edges,
            stats.non_manifold_edges,
            stats.signed_volume
        );
        assert!(stats.triangles > 0);
        assert!(stats.is_closed);
        assert_eq!(stats.boundary_edges, 0);
        assert!(stats.signed_volume > 0.35);
        assert!(hausdorff < 0.05, "mixed thin wall Hausdorff {hausdorff}");
    }

    #[test]
    fn mixed_level_open_boundary_is_reported() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };
        let graph = sphere_graph();
        let config = AdaptiveConfig {
            bounds: graph.bounds(),
            base_cells: 8,
            max_depth: 1,
            band_cells: 2.0,
            max_tiles: 100,
        };
        let mut adaptive = sample_adaptive(&sampler, &graph, &config).unwrap();
        for tile in &mut adaptive.tiles {
            let [nx, ny, nz] = tile.spec.dims;
            let (nx, ny) = (nx as usize, ny as usize);
            for z in 0..nz as usize {
                let world_z = tile.spec.origin[2] + z as f32 * tile.spec.cell;
                for y in 0..ny {
                    for x in 0..nx {
                        if world_z > 0.0 {
                            let index = x + nx * (y + ny * z);
                            tile.values[index] = tile.values[index].abs().max(0.1);
                        }
                    }
                }
            }
        }
        let mesh = crate::mesh_adaptive_mixed_dc(&graph, &adaptive).unwrap();
        let stats = mesh.stats();
        eprintln!(
            "mixed open boundary: verts={} tris={} closed={} boundary={} nonmanifold={}",
            stats.vertices,
            stats.triangles,
            stats.is_closed,
            stats.boundary_edges,
            stats.non_manifold_edges
        );
        assert!(stats.triangles > 0);
        assert!(!stats.is_closed);
        assert!(stats.boundary_edges > 0);
    }

    #[test]
    fn mixed_level_clipped_bounds_open_boundary() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };
        let graph = sphere_graph();
        let config = AdaptiveConfig {
            bounds: zeroset_core::Aabb::new(Vec3::new(-1.2, -1.2, -1.2), Vec3::new(1.2, 1.2, 0.0)),
            base_cells: 8,
            max_depth: 2,
            band_cells: 2.0,
            max_tiles: 12,
        };
        let adaptive = sample_adaptive(&sampler, &graph, &config).unwrap();
        let mesh = crate::mesh_adaptive_mixed_dc(&graph, &adaptive).unwrap();
        let stats = mesh.stats();
        eprintln!(
            "mixed clipped bounds: truncated={} verts={} tris={} closed={} boundary={} nonmanifold={}",
            adaptive.truncated,
            stats.vertices,
            stats.triangles,
            stats.is_closed,
            stats.boundary_edges,
            stats.non_manifold_edges
        );
        assert!(stats.triangles > 0);
        assert!(!stats.is_closed);
        assert!(stats.boundary_edges > 0);
        let capped = mesh.cap_boundary_loops();
        let capped_stats = capped.stats();
        eprintln!(
            "mixed clipped bounds capped: verts={} tris={} closed={} boundary={}",
            capped_stats.vertices,
            capped_stats.triangles,
            capped_stats.is_closed,
            capped_stats.boundary_edges
        );
        assert!(capped_stats.is_closed);
        assert_eq!(capped_stats.boundary_edges, 0);
    }

    #[test]
    fn mixed_level_torus_is_closed() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };
        let mut graph = FieldGraph::new();
        let node = graph.torus(Vec3::ZERO, Vec3::Z, 1.0, 0.35);
        graph.set_root(node);
        let config = AdaptiveConfig {
            bounds: graph.bounds(),
            base_cells: 8,
            max_depth: 2,
            band_cells: 2.0,
            max_tiles: 12,
        };
        let adaptive = sample_adaptive(&sampler, &graph, &config).unwrap();
        let mesh = crate::mesh_adaptive_mixed_dc(&graph, &adaptive).unwrap();
        let stats = mesh.stats();
        let refined = crate::mesh_adaptive_refined(&graph, &adaptive).unwrap();
        let hausdorff = refined.hausdorff_distance(&mesh, 3);
        eprintln!(
            "mixed torus: truncated={} verts={} tris={} closed={} boundary={} nonmanifold={} volume={} hausdorff={hausdorff}",
            adaptive.truncated,
            stats.vertices,
            stats.triangles,
            stats.is_closed,
            stats.boundary_edges,
            stats.non_manifold_edges,
            stats.signed_volume
        );
        assert!(stats.triangles > 0);
        assert!(stats.is_closed);
        assert_eq!(stats.boundary_edges, 0);
        assert!(stats.signed_volume > 2.2);
        assert!(hausdorff < 0.08, "mixed torus Hausdorff {hausdorff}");
    }

    // The raw truncated (`max_tiles`) mixed stitcher can emit non-manifold
    // transition edges, so this case pins the uniform mixed-DC path. The
    // truncated counterpart is covered by the fallback regression below.
    #[test]
    fn multi_hole_plate_is_closed() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };
        let mut graph = FieldGraph::new();
        let mut root = graph.rounded_box(Vec3::ZERO, Vec3::new(1.0, 1.0, 0.25), 0.0);
        for x in [-0.5f32, 0.0, 0.5] {
            let hole = graph.cylinder(Vec3::new(x, 0.0, 0.0), Vec3::Y, 0.15, 1.0);
            root = graph.subtract(root, hole);
        }
        graph.set_root(root);
        let config = AdaptiveConfig {
            bounds: graph.bounds(),
            base_cells: 8,
            max_depth: 2,
            band_cells: 2.0,
            max_tiles: 10_000,
        };
        let adaptive = sample_adaptive(&sampler, &graph, &config).unwrap();
        let mesh = crate::mesh_adaptive_mixed_dc(&graph, &adaptive).unwrap();
        let stats = mesh.stats();
        let refined = crate::mesh_adaptive_refined(&graph, &adaptive).unwrap();
        let hausdorff = refined.hausdorff_distance(&mesh, 3);
        eprintln!(
            "mixed multi-hole plate: truncated={} verts={} tris={} closed={} boundary={} nonmanifold={} volume={} hausdorff={hausdorff}",
            adaptive.truncated,
            stats.vertices,
            stats.triangles,
            stats.is_closed,
            stats.boundary_edges,
            stats.non_manifold_edges,
            stats.signed_volume
        );
        assert!(stats.triangles > 0);
        assert!(!adaptive.truncated);
        assert!(stats.is_closed);
        assert_eq!(stats.boundary_edges, 0);
        assert!(stats.signed_volume > 1.5);
        assert!(stats.signed_volume < 2.0);
        assert!(
            refined.stats().signed_volume > 1.5,
            "refined SN volume {}",
            refined.stats().signed_volume
        );
        assert!(hausdorff < 0.1, "mixed multi-hole Hausdorff {hausdorff}");
    }

    #[test]
    fn truncated_multi_hole_nonmanifold_falls_back_to_refined() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };
        let mut graph = FieldGraph::new();
        let mut root = graph.rounded_box(Vec3::ZERO, Vec3::new(1.0, 1.0, 0.25), 0.0);
        for x in [-0.5f32, 0.0, 0.5] {
            let hole = graph.cylinder(Vec3::new(x, 0.0, 0.0), Vec3::Y, 0.15, 1.0);
            root = graph.subtract(root, hole);
        }
        graph.set_root(root);
        let config = AdaptiveConfig {
            bounds: graph.bounds(),
            base_cells: 8,
            max_depth: 2,
            band_cells: 2.0,
            max_tiles: 12,
        };
        let adaptive = sample_adaptive(&sampler, &graph, &config).unwrap();
        assert!(adaptive.truncated, "expected truncated leaves");
        let mesh = crate::mesh_adaptive_mixed_dc(&graph, &adaptive).unwrap();
        let stats = mesh.stats();
        let refined = crate::mesh_adaptive_refined_dc(&graph, &adaptive).unwrap();
        let refined_stats = refined.stats();
        eprintln!(
            "truncated multi-hole fallback: verts={} tris={} closed={} boundary={} nonmanifold={} volume={}",
            stats.vertices,
            stats.triangles,
            stats.is_closed,
            stats.boundary_edges,
            stats.non_manifold_edges,
            stats.signed_volume
        );
        assert!(stats.is_closed);
        assert_eq!(stats.boundary_edges, 0);
        assert_eq!(stats.non_manifold_edges, 0);
        // Raw mixed output was 1,412 verts with 158 non-manifold edges; the
        // fallback emits the refined 4,520-vertex surface.
        assert!(
            stats.vertices > 4000,
            "fallback vertices {}",
            stats.vertices
        );
        assert!(stats.signed_volume > 1.5 && stats.signed_volume < 1.8);
        assert!((stats.signed_volume - refined_stats.signed_volume).abs() < 1e-6);
    }

    #[test]
    fn mixed_level_capsule_and_cylinder_are_closed() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };
        for (name, expected_volume) in [("capsule", 0.4f64), ("cylinder", 1.0f64)] {
            let mut graph = FieldGraph::new();
            let node = if name == "capsule" {
                graph.capsule(Vec3::new(-0.5, 0.0, 0.0), Vec3::new(0.5, 0.0, 0.0), 0.35)
            } else {
                graph.cylinder(Vec3::ZERO, Vec3::Y, 0.5, 0.75)
            };
            graph.set_root(node);
            let config = AdaptiveConfig {
                bounds: graph.bounds(),
                base_cells: 8,
                max_depth: 2,
                band_cells: 2.0,
                max_tiles: 12,
            };
            let adaptive = sample_adaptive(&sampler, &graph, &config).unwrap();
            let mesh = crate::mesh_adaptive_mixed_dc(&graph, &adaptive).unwrap();
            let stats = mesh.stats();
            eprintln!(
                "mixed {name}: truncated={} verts={} tris={} closed={} boundary={} nonmanifold={} volume={}",
                adaptive.truncated,
                stats.vertices,
                stats.triangles,
                stats.is_closed,
                stats.boundary_edges,
                stats.non_manifold_edges,
                stats.signed_volume
            );
            assert!(stats.triangles > 0, "{name}");
            assert!(stats.is_closed, "{name} not closed");
            assert_eq!(stats.boundary_edges, 0, "{name}");
            assert!(
                stats.signed_volume > expected_volume,
                "{name} volume {}",
                stats.signed_volume
            );
        }
    }

    #[test]
    fn adaptive_mesh_is_closed_and_outward() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };

        let graph = sphere_graph();
        let config = AdaptiveConfig {
            bounds: graph.bounds(),
            base_cells: 8,
            max_depth: 2,
            band_cells: 2.0,
            max_tiles: 4096,
        };
        let adaptive = sample_adaptive(&sampler, &graph, &config).unwrap();
        let mesh = crate::mesh_adaptive(&adaptive).unwrap();
        let stats = mesh.stats();

        assert!(stats.triangles > 0);
        assert_eq!(stats.boundary_edges, 0, "adaptive mesh has cracks");
        assert_eq!(stats.non_manifold_edges, 0);
        assert!(stats.is_closed);
        assert!(stats.signed_volume > 0.0);
        for position in &mesh.positions {
            let radius = Vec3::from_array(*position).length();
            assert!((radius - 1.0).abs() < 0.15, "radius {radius}");
        }
    }

    #[test]
    fn adaptive_tiles_preserve_world_values() {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };

        let graph = sphere_graph();
        let config = AdaptiveConfig {
            bounds: graph.bounds(),
            base_cells: 8,
            max_depth: 1,
            band_cells: 2.0,
            max_tiles: 4096,
        };
        let adaptive = sample_adaptive(&sampler, &graph, &config).unwrap();
        let tile = adaptive.tiles.first().unwrap();

        for (index, value) in tile.values.iter().enumerate().take(16) {
            let nx = tile.spec.dims[0] as usize;
            let ny = tile.spec.dims[1] as usize;
            let x = index % nx;
            let y = (index / nx) % ny;
            let z = index / (nx * ny);
            let local = Vec3::from_array(tile.spec.position(x as u32, y as u32, z as u32));
            let world = local + adaptive.reference;
            let cpu = graph.evaluate(world);
            assert!(
                (value - cpu).abs() < 1e-3,
                "gpu={value} cpu={cpu} at {world:?}"
            );
        }
    }
}
