//! Experimental mixed-level Surface Nets.
//!
//! Each leaf cell gets one vertex. Fine lattice edges are walked across the
//! whole sparse grid, and the four cells around an edge are mapped to their
//! containing leaf (coarse cells collapse several fine cells to one vertex).
//! Degenerate quads are dropped; this is a prototype for true transition
//! stitching, not a replacement for the refined fallback.

use std::collections::{HashMap, HashSet};

use glam::{Mat3, Vec3};
use zeroset_core::{FieldGraph, Mesh};

use crate::{AdaptiveGrid, GpuError};

type Key = [i32; 3];

const CORNERS: [(i32, i32, i32); 8] = [
    (0, 0, 0),
    (1, 0, 0),
    (1, 1, 0),
    (0, 1, 0),
    (0, 0, 1),
    (1, 0, 1),
    (1, 1, 1),
    (0, 1, 1),
];

const EDGES: [(usize, usize); 12] = [
    (0, 1),
    (1, 2),
    (2, 3),
    (3, 0),
    (4, 5),
    (5, 6),
    (6, 7),
    (7, 4),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

pub fn mesh_adaptive_mixed(graph: &FieldGraph, grid: &AdaptiveGrid) -> Result<Mesh, GpuError> {
    mesh_adaptive_mixed_impl(graph, grid, false, true)
}

pub fn mesh_adaptive_mixed_dc(graph: &FieldGraph, grid: &AdaptiveGrid) -> Result<Mesh, GpuError> {
    mesh_adaptive_mixed_impl(graph, grid, true, true)
}

fn mesh_adaptive_mixed_impl(
    graph: &FieldGraph,
    grid: &AdaptiveGrid,
    qef: bool,
    allow_fallback: bool,
) -> Result<Mesh, GpuError> {
    let Some(finest) = grid.finest_cell() else {
        return Ok(Mesh::default());
    };
    if !finest.is_finite() || finest <= 0.0 {
        return Err(GpuError::InvalidAdaptive(
            "adaptive grid has an invalid finest cell".to_string(),
        ));
    }
    let origin = grid
        .tiles
        .iter()
        .map(|tile| Vec3::from_array(tile.spec.origin))
        .reduce(Vec3::min)
        .ok_or_else(|| GpuError::InvalidAdaptive("adaptive grid is empty".to_string()))?;
    let reference = grid.reference;

    let mut positions: Vec<Vec3> = Vec::new();
    // Whether each vertex belongs to a coarse leaf (factor > 1). Transition
    // quads are triangulated along the diagonal with the most fine endpoints so
    // several transition quads cannot all share the same coarse-coarse edge.
    let mut coarse_vertices: Vec<bool> = Vec::new();
    let mut fine_vertices: HashMap<Key, u32> = HashMap::new();
    for tile in &grid.tiles {
        let factor = (tile.spec.cell / finest).round().max(1.0) as i32;
        let base = ((Vec3::from_array(tile.spec.origin) - origin) / finest).round();
        let base = [base.x as i32, base.y as i32, base.z as i32];
        let [nx, ny, nz] = tile.spec.dims;
        let (nx, ny, nz) = (nx as i32, ny as i32, nz as i32);
        for z in 0..nz - 1 {
            for y in 0..ny - 1 {
                for x in 0..nx - 1 {
                    let mut values = [0.0f32; 8];
                    let mut negative = 0u8;
                    for (index, (dx, dy, dz)) in CORNERS.iter().enumerate() {
                        let value = tile.values[(x + dx + nx * (y + dy + ny * (z + dz))) as usize];
                        values[index] = value;
                        if value < 0.0 {
                            negative |= 1 << index;
                        }
                    }
                    if negative == 0 || negative == 0xFF {
                        continue;
                    }
                    let mut sum = Vec3::ZERO;
                    let mut centroid = Vec3::ZERO;
                    let mut ata = Mat3::ZERO;
                    let mut atb = Vec3::ZERO;
                    let mut crossings = 0u32;
                    let mut planes: Vec<(Vec3, f32)> = Vec::new();
                    for (a, b) in EDGES {
                        let va = values[a];
                        let vb = values[b];
                        if (va < 0.0) == (vb < 0.0) {
                            continue;
                        }
                        let denominator = va - vb;
                        let t = if denominator.abs() > f32::EPSILON {
                            (va / denominator).clamp(0.0, 1.0)
                        } else {
                            0.5
                        };
                        let pa = world_position(
                            reference, origin, finest, factor, base, x, y, z, CORNERS[a],
                        );
                        let pb = world_position(
                            reference, origin, finest, factor, base, x, y, z, CORNERS[b],
                        );
                        let intersection = pa.lerp(pb, t);
                        sum += intersection;
                        centroid += intersection;
                        let normal = graph.gradient(intersection);
                        if normal.length_squared() > 1e-12 {
                            let normal = normal.normalize();
                            let offset = normal.dot(intersection);
                            planes.push((normal, offset));
                            ata += Mat3::from_cols(
                                normal * normal.x,
                                normal * normal.y,
                                normal * normal.z,
                            );
                            atb += normal * offset;
                        }
                        crossings += 1;
                    }
                    if crossings == 0 {
                        continue;
                    }
                    let mut unique: Vec<(Vec3, f32)> = Vec::new();
                    for (normal, offset) in &planes {
                        if !unique
                            .iter()
                            .any(|(existing, _)| existing.dot(*normal) > 0.995)
                        {
                            unique.push((*normal, *offset));
                        }
                    }
                    let mut feature = false;
                    for first in 0..unique.len() {
                        for second in first + 1..unique.len() {
                            let (n1, d1) = unique[first];
                            let (n2, d2) = unique[second];
                            let cosine = n1.dot(n2);
                            if cosine > 0.5 {
                                continue;
                            }
                            let determinant = 1.0 - cosine * cosine;
                            if determinant.abs() < 1e-4 {
                                continue;
                            }
                            let a = (d1 - cosine * d2) / determinant;
                            let b = (d2 - cosine * d1) / determinant;
                            let point = n1 * a + n2 * b;
                            let direction = n1.cross(n2).normalize_or_zero();
                            if direction.length_squared() < 0.5 {
                                continue;
                            }
                            let projector = Mat3::IDENTITY
                                - Mat3::from_cols(
                                    direction * direction.x,
                                    direction * direction.y,
                                    direction * direction.z,
                                );
                            let weight = 2.0 + 4.0 * (1.0 - cosine);
                            feature = true;
                            ata += projector * weight;
                            atb += projector * point * weight;
                        }
                    }
                    if unique.len() >= 3 {
                        let (n1, d1) = unique[0];
                        let (n2, d2) = unique[1];
                        let (n3, d3) = unique[2];
                        if n1.dot(n2) < 0.5 && n1.dot(n3) < 0.5 && n2.dot(n3) < 0.5 {
                            let normals = Mat3::from_cols(n1, n2, n3);
                            let determinant = normals.determinant();
                            if determinant.abs() > 1e-4 {
                                let corner = normals.inverse() * Vec3::new(d1, d2, d3);
                                if corner.is_finite() {
                                    let weight = 2.0
                                        + 4.0 * (1.0 - n1.dot(n2).max(n1.dot(n3)).max(n2.dot(n3)));
                                    ata += Mat3::IDENTITY * weight;
                                    atb += corner * weight;
                                }
                            }
                        }
                    }
                    let mass_point = sum / crossings as f32;
                    let position = if qef {
                        centroid /= crossings as f32;
                        let lambda = if feature { 1e-4 } else { 1e-3 };
                        ata += Mat3::IDENTITY * lambda;
                        atb += centroid * lambda;
                        let determinant = ata.determinant();
                        let mut vertex = if determinant.abs() > 1e-8 {
                            ata.inverse() * atb
                        } else {
                            centroid
                        };
                        if !vertex.is_finite() {
                            vertex = centroid;
                        }
                        if !feature {
                            let offset =
                                (vertex - mass_point).length() / (factor as f32 * finest).max(1e-6);
                            let blend = if factor > 1 {
                                (0.7 + 0.2 * offset.clamp(0.0, 1.0)).min(0.95)
                            } else {
                                1.0
                            };
                            vertex = vertex.lerp(mass_point, blend);
                        }
                        let cell_min = reference
                            + origin
                            + Vec3::new(
                                (base[0] + x * factor) as f32,
                                (base[1] + y * factor) as f32,
                                (base[2] + z * factor) as f32,
                            ) * finest;
                        vertex.clamp(
                            cell_min,
                            cell_min + Vec3::splat((factor as f32 * finest).max(finest)),
                        )
                    } else {
                        mass_point
                    };
                    let vertex = positions.len() as u32;
                    positions.push(position);
                    coarse_vertices.push(factor > 1);
                    for fz in 0..factor {
                        for fy in 0..factor {
                            for fx in 0..factor {
                                fine_vertices.insert(
                                    [
                                        base[0] + x * factor + fx,
                                        base[1] + y * factor + fy,
                                        base[2] + z * factor + fz,
                                    ],
                                    vertex,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    if fine_vertices.is_empty() {
        return Ok(Mesh::default());
    }

    let mut fine_values: HashMap<Key, f32> = HashMap::new();
    let mut value_at = |key: Key| -> f32 {
        *fine_values.entry(key).or_insert_with(|| {
            let point = reference
                + origin
                + Vec3::new(key[0] as f32, key[1] as f32, key[2] as f32) * finest;
            graph.evaluate(point)
        })
    };

    let mut indices: Vec<u32> = Vec::new();
    let mut edges_seen: HashSet<(i32, i32, i32, u8)> = HashSet::new();
    let mut quads_seen: HashSet<Vec<u32>> = HashSet::new();
    let mut triangles_seen: HashSet<[u32; 3]> = HashSet::new();
    let cells: Vec<Key> = fine_vertices.keys().copied().collect();
    for cell in cells {
        for axis in 0..3u8 {
            let start = cell;
            if !edges_seen.insert((start[0], start[1], start[2], axis)) {
                continue;
            }
            let end = match axis {
                0 => [start[0] + 1, start[1], start[2]],
                1 => [start[0], start[1] + 1, start[2]],
                _ => [start[0], start[1], start[2] + 1],
            };
            let value_start = value_at(start);
            let value_end = value_at(end);
            if (value_start < 0.0) == (value_end < 0.0) {
                continue;
            }
            let surrounding = match axis {
                0 => [
                    [start[0], start[1] - 1, start[2] - 1],
                    [start[0], start[1], start[2] - 1],
                    [start[0], start[1], start[2]],
                    [start[0], start[1] - 1, start[2]],
                ],
                1 => [
                    [start[0] - 1, start[1], start[2] - 1],
                    [start[0] - 1, start[1], start[2]],
                    [start[0], start[1], start[2]],
                    [start[0], start[1], start[2] - 1],
                ],
                _ => [
                    [start[0] - 1, start[1] - 1, start[2]],
                    [start[0], start[1] - 1, start[2]],
                    [start[0], start[1], start[2]],
                    [start[0] - 1, start[1], start[2]],
                ],
            };
            let mut ring = [0u32; 4];
            let mut complete = true;
            for (slot, key) in surrounding.into_iter().enumerate() {
                match fine_vertices.get(&key) {
                    Some(vertex) => ring[slot] = *vertex,
                    None => {
                        complete = false;
                        break;
                    }
                }
            }
            if !complete {
                continue;
            }
            // Collapse adjacent coarse/fine duplicates. A repeated corner that is
            // not adjacent is a bow-tie (the quad folded over); emitting a fan for
            // it creates the non-manifold transition edges.
            let mut quad: Vec<u32> = ring.to_vec();
            let mut changed = true;
            while changed && quad.len() > 2 {
                changed = false;
                let mut collapsed = Vec::with_capacity(quad.len());
                for index in 0..quad.len() {
                    let previous = quad[(index + quad.len() - 1) % quad.len()];
                    if quad[index] == previous {
                        changed = true;
                    } else {
                        collapsed.push(quad[index]);
                    }
                }
                quad = collapsed;
            }
            if quad.len() < 3 {
                continue;
            }
            let mut unique = quad.clone();
            unique.sort_unstable();
            unique.dedup();
            if unique.len() != quad.len() {
                continue;
            }
            // Multiple fine edges along a coarse/fine transition can collapse onto
            // the same coarse-vertex ring. Keep only one quad for that ring.
            let mut quad_key = quad.clone();
            quad_key.sort_unstable();
            if !quads_seen.insert(quad_key) {
                continue;
            }
            if value_start > 0.0 {
                quad.reverse();
            }
            // Prefer the fine-fine diagonal. If a transition quad's two coarse
            // vertices are opposite corners, fanning along the coarse-coarse
            // diagonal lets neighbouring quads emit four triangles on that
            // edge. Rotating the fan to the other diagonal (and falling back to
            // the lone fine vertex when only one exists) keeps transition edges
            // manifold more often.
            let diagonal_score = |a: u32, b: u32| -> u8 {
                match (coarse_vertices[a as usize], coarse_vertices[b as usize]) {
                    (false, false) => 2,
                    (true, true) => 0,
                    _ => 1,
                }
            };
            let root = if quad.len() == 4
                && diagonal_score(quad[1], quad[3]) > diagonal_score(quad[0], quad[2])
            {
                1
            } else {
                0
            };
            for index in 1..quad.len() - 1 {
                let triangle = [
                    quad[root],
                    quad[(root + index) % quad.len()],
                    quad[(root + index + 1) % quad.len()],
                ];
                if triangle[0] != triangle[1]
                    && triangle[1] != triangle[2]
                    && triangle[0] != triangle[2]
                {
                    let mut triangle_key = triangle;
                    triangle_key.sort_unstable();
                    if triangles_seen.insert(triangle_key) {
                        indices.extend_from_slice(&triangle);
                    }
                }
            }
        }
    }
    if positions.is_empty() || indices.is_empty() {
        return Ok(Mesh::default());
    }

    for triangle in indices.chunks_exact_mut(3) {
        let a = positions[triangle[0] as usize];
        let b = positions[triangle[1] as usize];
        let c = positions[triangle[2] as usize];
        let face = (b - a).cross(c - a);
        let centroid = (a + b + c) / 3.0;
        if face.dot(graph.gradient(centroid)) < 0.0 {
            triangle.swap(1, 2);
        }
    }

    let mut normals = vec![Vec3::ZERO; positions.len()];
    for triangle in indices.chunks_exact(3) {
        let a = positions[triangle[0] as usize];
        let b = positions[triangle[1] as usize];
        let c = positions[triangle[2] as usize];
        let face = (b - a).cross(c - a);
        normals[triangle[0] as usize] += face;
        normals[triangle[1] as usize] += face;
        normals[triangle[2] as usize] += face;
    }
    for normal in &mut normals {
        *normal = normal.normalize_or_zero();
        if normal.length_squared() < 0.5 {
            *normal = Vec3::Y;
        }
    }
    let mesh = Mesh {
        positions: positions
            .iter()
            .map(|position| position.to_array())
            .collect(),
        normals: normals.iter().map(|normal| normal.to_array()).collect(),
        indices,
    };
    // The raw mixed-level stitcher can still fold transition quads into
    // non-manifold edges (e.g. the truncated three-hole plate). Fall back to the
    // refined path for truncated grids so callers always receive a manifold
    // surface; uniform mixed grids keep the true mixed result.
    if allow_fallback && grid.truncated {
        let stats = mesh.stats();
        if stats.non_manifold_edges > 0 {
            return if qef {
                crate::mesh_adaptive_refined_dc(graph, grid)
            } else {
                crate::mesh_adaptive_refined(graph, grid)
            };
        }
    }
    Ok(mesh)
}

#[allow(clippy::too_many_arguments)]
fn world_position(
    reference: Vec3,
    origin: Vec3,
    cell: f32,
    factor: i32,
    base: Key,
    x: i32,
    y: i32,
    z: i32,
    corner: (i32, i32, i32),
) -> Vec3 {
    reference
        + origin
        + Vec3::new(
            (base[0] + (x + corner.0) * factor) as f32,
            (base[1] + (y + corner.1) * factor) as f32,
            (base[2] + (z + corner.2) * factor) as f32,
        ) * cell
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AdaptiveConfig, GpuSampler, sample_adaptive};

    fn truncated_multi_hole() -> Option<(FieldGraph, AdaptiveGrid)> {
        let sampler = match GpuSampler::new() {
            Ok(sampler) => sampler,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return None;
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
        Some((graph, adaptive))
    }

    #[test]
    fn truncated_multi_hole_raw_transition_is_better() {
        let Some((graph, adaptive)) = truncated_multi_hole() else {
            return;
        };
        assert!(adaptive.truncated, "expected truncated leaves");
        // Bypass the public fallback to see what the raw mixed stitcher emits.
        let mesh = mesh_adaptive_mixed_impl(&graph, &adaptive, true, false).unwrap();
        let stats = mesh.stats();
        eprintln!(
            "raw truncated mixed: verts={} tris={} closed={} boundary={} nonmanifold={}",
            stats.vertices,
            stats.triangles,
            stats.is_closed,
            stats.boundary_edges,
            stats.non_manifold_edges
        );
        assert!(stats.triangles > 0);
        assert!(stats.vertices < 4000, "raw path unexpectedly fell back");
        // The pre-fan fix raw output had 158 non-manifold edges; the fine-fine
        // diagonal should not make transition stitching worse.
        assert!(
            stats.non_manifold_edges <= 158,
            "raw non-manifold edges {}",
            stats.non_manifold_edges
        );
        assert!(!stats.is_closed || stats.non_manifold_edges == 0);
    }
}
