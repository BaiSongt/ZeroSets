//! Crack-free narrow-band meshing over an adaptive occupancy grid.
//!
//! `sample_adaptive` produces disjoint leaf tiles at one common cell size; the
//! tiles only cover the zero-set neighbourhood. This module treats them as a
//! sparse set of cells on one global lattice and runs Surface Nets with global
//! cell/edge keys. Because every surviving cell uses the same cell size and
//! shared corners resolve to the same lattice points, quads at tile boundaries
//! connect across tiles instead of leaving T-junction cracks.

use std::collections::{HashMap, HashSet};

use glam::{Mat3, Vec3};

use zeroset_core::{FieldGraph, Mesh};

use crate::{AdaptiveGrid, GpuError, TileSpec, TileValues};

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

/// Mesh an adaptive grid with one consistent finest cell size.
///
/// The result is in world space (`AdaptiveGrid::reference + local`). The
/// function requires all leaf tiles to share a cell size; `sample_adaptive`
/// produces that when `max_tiles` is not exhausted.
pub fn mesh_adaptive_dc(graph: &FieldGraph, grid: &AdaptiveGrid) -> Result<Mesh, GpuError> {
    let mut mesh = mesh_adaptive(grid)?;
    if mesh.positions.is_empty() || grid.tiles.is_empty() {
        return Ok(mesh);
    }

    let cell = grid.tiles[0].spec.cell;
    let origin = grid
        .tiles
        .iter()
        .map(|tile| Vec3::from_array(tile.spec.origin))
        .reduce(Vec3::min)
        .expect("tiles is non-empty");

    let mut corners: HashMap<Key, f32> = HashMap::new();
    for tile in &grid.tiles {
        let base = ((Vec3::from_array(tile.spec.origin) - origin) / cell).round();
        let base = [base.x as i32, base.y as i32, base.z as i32];
        let [nx, ny, nz] = tile.spec.dims;
        let (nx, ny, nz) = (nx as i32, ny as i32, nz as i32);
        for z in 0..nz {
            for y in 0..ny {
                for x in 0..nx {
                    corners
                        .entry([base[0] + x, base[1] + y, base[2] + z])
                        .or_insert(tile.values[(x + nx * (y + ny * z)) as usize]);
                }
            }
        }
    }

    let mut seen = HashSet::new();
    let mut positions: Vec<Vec3> = Vec::with_capacity(mesh.positions.len());
    for tile in &grid.tiles {
        let base = ((Vec3::from_array(tile.spec.origin) - origin) / cell).round();
        let base = [base.x as i32, base.y as i32, base.z as i32];
        let [nx, ny, nz] = tile.spec.dims;
        let (nx, ny, nz) = (nx as i32, ny as i32, nz as i32);
        for z in 0..nz - 1 {
            for y in 0..ny - 1 {
                for x in 0..nx - 1 {
                    let cell_key = [base[0] + x, base[1] + y, base[2] + z];
                    if !seen.insert(cell_key) {
                        continue;
                    }
                    let mut values = [0.0f32; 8];
                    let mut negative = 0u8;
                    for (index, (dx, dy, dz)) in CORNERS.iter().enumerate() {
                        let value = *corners
                            .get(&[cell_key[0] + dx, cell_key[1] + dy, cell_key[2] + dz])
                            .ok_or_else(|| {
                                GpuError::InvalidAdaptive(
                                    "adaptive DC corner is missing".to_string(),
                                )
                            })?;
                        values[index] = value;
                        if value < 0.0 {
                            negative |= 1 << index;
                        }
                    }
                    if negative == 0 || negative == 0xFF {
                        continue;
                    }

                    let mut ata = Mat3::ZERO;
                    let mut atb = Vec3::ZERO;
                    let mut centroid = Vec3::ZERO;
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
                        let pa = lattice_position(origin, cell, cell_key, CORNERS[a]);
                        let pb = lattice_position(origin, cell, cell_key, CORNERS[b]);
                        let intersection = pa.lerp(pb, t);
                        centroid += intersection;
                        crossings += 1;
                        let normal = graph.gradient(grid.reference + intersection);
                        if normal.length_squared() > 1e-12 {
                            let normal = normal.normalize();
                            planes.push((normal, normal.dot(intersection)));
                            ata += Mat3::from_cols(
                                normal * normal.x,
                                normal * normal.y,
                                normal * normal.z,
                            );
                            atb += normal * normal.dot(intersection);
                        }
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
                    centroid /= crossings as f32;
                    let lambda = if unique.len() >= 2 && unique[0].0.dot(unique[1].0) < 0.5 {
                        1e-4
                    } else {
                        1e-3
                    };
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
                    let cell_min = origin
                        + Vec3::new(cell_key[0] as f32, cell_key[1] as f32, cell_key[2] as f32)
                            * cell;
                    vertex = vertex.clamp(cell_min, cell_min + Vec3::splat(cell));
                    positions.push(vertex);
                }
            }
        }
    }

    if positions.len() != mesh.positions.len() {
        return Err(GpuError::InvalidAdaptive(format!(
            "adaptive DC vertex count mismatch: {} vs {}",
            positions.len(),
            mesh.positions.len()
        )));
    }
    for (index, position) in positions.iter().enumerate() {
        mesh.positions[index] = (*position + grid.reference).to_array();
    }

    let mut normals = vec![Vec3::ZERO; mesh.positions.len()];
    for triangle in mesh.indices.chunks_exact(3) {
        let a = Vec3::from_array(mesh.positions[triangle[0] as usize]);
        let b = Vec3::from_array(mesh.positions[triangle[1] as usize]);
        let c = Vec3::from_array(mesh.positions[triangle[2] as usize]);
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
    mesh.normals = normals.iter().map(|normal| normal.to_array()).collect();
    Ok(mesh)
}

pub fn mesh_adaptive(grid: &AdaptiveGrid) -> Result<Mesh, GpuError> {
    if grid.tiles.is_empty() {
        return Ok(Mesh::default());
    }

    let cell = grid.tiles[0].spec.cell;
    if !cell.is_finite() || cell <= 0.0 {
        return Err(GpuError::InvalidAdaptive(
            "adaptive tiles have an invalid cell size".to_string(),
        ));
    }
    for tile in &grid.tiles {
        if (tile.spec.cell - cell).abs() > cell * 1e-4 {
            return Err(GpuError::InvalidAdaptive(
                "mesh_adaptive requires uniform leaf cell sizes; the adaptive grid was truncated"
                    .to_string(),
            ));
        }
    }

    let origin = grid
        .tiles
        .iter()
        .map(|tile| Vec3::from_array(tile.spec.origin))
        .reduce(Vec3::min)
        .expect("tiles is non-empty");

    let mut corners: HashMap<Key, f32> = HashMap::new();
    let mut cell_vertices: HashMap<Key, u32> = HashMap::new();
    let mut positions: Vec<Vec3> = Vec::new();

    for tile in &grid.tiles {
        let base = ((Vec3::from_array(tile.spec.origin) - origin) / cell).round();
        let base = [base.x as i32, base.y as i32, base.z as i32];
        let [nx, ny, nz] = tile.spec.dims;
        let nx = nx as i32;
        let ny = ny as i32;
        let nz = nz as i32;

        for z in 0..nz {
            for y in 0..ny {
                for x in 0..nx {
                    let value = tile.values[(x + nx * (y + ny * z)) as usize];
                    corners
                        .entry([base[0] + x, base[1] + y, base[2] + z])
                        .or_insert(value);
                }
            }
        }
    }

    for tile in &grid.tiles {
        let base = ((Vec3::from_array(tile.spec.origin) - origin) / cell).round();
        let base = [base.x as i32, base.y as i32, base.z as i32];
        let [nx, ny, nz] = tile.spec.dims;
        let nx = nx as i32;
        let ny = ny as i32;
        let nz = nz as i32;

        for z in 0..nz - 1 {
            for y in 0..ny - 1 {
                for x in 0..nx - 1 {
                    let cell_key = [base[0] + x, base[1] + y, base[2] + z];
                    if cell_vertices.contains_key(&cell_key) {
                        continue;
                    }

                    let mut values = [0.0f32; 8];
                    let mut negative = 0u8;
                    for (index, (dx, dy, dz)) in CORNERS.iter().enumerate() {
                        let value = *corners
                            .get(&[cell_key[0] + dx, cell_key[1] + dy, cell_key[2] + dz])
                            .ok_or_else(|| {
                                GpuError::InvalidAdaptive(
                                    "adaptive mesh corner is missing".to_string(),
                                )
                            })?;
                        values[index] = value;
                        if value < 0.0 {
                            negative |= 1 << index;
                        }
                    }
                    if negative == 0 || negative == 0xFF {
                        continue;
                    }

                    let mut sum = Vec3::ZERO;
                    let mut crossings = 0u32;
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
                        let pa = lattice_position(origin, cell, cell_key, CORNERS[a]);
                        let pb = lattice_position(origin, cell, cell_key, CORNERS[b]);
                        sum += pa.lerp(pb, t);
                        crossings += 1;
                    }
                    if crossings == 0 {
                        continue;
                    }
                    let index = positions.len() as u32;
                    positions.push(sum / crossings as f32);
                    cell_vertices.insert(cell_key, index);
                }
            }
        }
    }

    let mut indices: Vec<u32> = Vec::new();
    let mut edges_seen: HashSet<(i32, i32, i32, u8)> = HashSet::new();
    let mut missing_quads = 0usize;

    for cell_key in cell_vertices.keys() {
        let [cx, cy, cz] = *cell_key;
        let mut cell_values = [0.0f32; 8];
        for (index, (dx, dy, dz)) in CORNERS.iter().enumerate() {
            let value = *corners.get(&[cx + dx, cy + dy, cz + dz]).ok_or_else(|| {
                GpuError::InvalidAdaptive("adaptive mesh corner is missing".to_string())
            })?;
            cell_values[index] = value;
        }

        // X edges: start corners (0,0,0), (0,1,0), (0,1,1), (0,0,1).
        for (start, end) in [(0usize, 1usize), (3, 2), (7, 6), (4, 5)] {
            let (dy, dz) = (CORNERS[start].1, CORNERS[start].2);
            emit_edge(
                cell_values[start],
                cell_values[end],
                [cx, cy + dy, cz + dz],
                0,
                &cell_vertices,
                &mut edges_seen,
                &mut indices,
                &mut missing_quads,
            );
        }
        // Y edges: start corners (0,0,0), (1,0,0), (1,0,1), (0,0,1).
        for (start, end) in [(0usize, 3usize), (1, 2), (5, 6), (4, 7)] {
            let (dx, dz) = (CORNERS[start].0, CORNERS[start].2);
            emit_edge(
                cell_values[start],
                cell_values[end],
                [cx + dx, cy, cz + dz],
                1,
                &cell_vertices,
                &mut edges_seen,
                &mut indices,
                &mut missing_quads,
            );
        }
        // Z edges: start corners (0,0,0), (1,0,0), (1,1,0), (0,1,0).
        for (start, end) in [(0usize, 4usize), (1, 5), (2, 6), (3, 7)] {
            let (dx, dy) = (CORNERS[start].0, CORNERS[start].1);
            emit_edge(
                cell_values[start],
                cell_values[end],
                [cx + dx, cy + dy, cz],
                2,
                &cell_vertices,
                &mut edges_seen,
                &mut indices,
                &mut missing_quads,
            );
        }
    }

    if positions.is_empty() || indices.is_empty() {
        return Ok(Mesh::default());
    }

    // Orient the closed mesh outward.
    let mut volume = 0.0f64;
    for triangle in indices.chunks_exact(3) {
        let a = positions[triangle[0] as usize];
        let b = positions[triangle[1] as usize];
        let c = positions[triangle[2] as usize];
        volume += (a.dot(b.cross(c)) / 6.0) as f64;
    }
    if volume < 0.0 {
        for triangle in indices.chunks_exact_mut(3) {
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

    let reference = grid.reference;
    for normal in &mut normals {
        *normal = normal.normalize_or_zero();
        if normal.length_squared() < 0.5 {
            *normal = Vec3::Y;
        }
    }

    let _ = missing_quads;

    Ok(Mesh {
        positions: positions
            .iter()
            .map(|position| (*position + reference).to_array())
            .collect(),
        normals: normals.iter().map(|normal| normal.to_array()).collect(),
        indices,
    })
}

/// Mesh an adaptive grid even when `max_tiles` truncated it to mixed leaf
/// levels. Coarse leaves are resampled on the CPU at the finest leaf cell size,
/// then the uniform `mesh_adaptive` path is used, so the result stays
/// crack-free.
///
/// This is a fallback for truncated grids, not multi-resolution stitching:
/// resampling can be expensive when truncation happened at a shallow level.
pub fn refine_adaptive_grid(
    graph: &FieldGraph,
    grid: &AdaptiveGrid,
) -> Result<AdaptiveGrid, GpuError> {
    let Some(finest) = grid.finest_cell() else {
        return Ok(grid.clone());
    };
    let uniform = grid
        .tiles
        .iter()
        .all(|tile| (tile.spec.cell - finest).abs() <= finest * 1e-4);
    if uniform {
        return Ok(grid.clone());
    }

    let local_graph = graph.rebased(grid.reference);
    let mut tiles = Vec::with_capacity(grid.tiles.len());
    let mut refined_points = 0usize;
    const MAX_REFINED_POINTS: usize = 4_000_000;

    for tile in &grid.tiles {
        if (tile.spec.cell - finest).abs() <= finest * 1e-4 {
            tiles.push(tile.clone());
            continue;
        }

        let cells = [
            ((tile.spec.cell * (tile.spec.dims[0] - 1) as f32) / finest).round() as u32,
            ((tile.spec.cell * (tile.spec.dims[1] - 1) as f32) / finest).round() as u32,
            ((tile.spec.cell * (tile.spec.dims[2] - 1) as f32) / finest).round() as u32,
        ];
        let dims = [cells[0] + 1, cells[1] + 1, cells[2] + 1];
        let count = dims[0] as usize * dims[1] as usize * dims[2] as usize;
        refined_points += count;
        if refined_points > MAX_REFINED_POINTS {
            return Err(GpuError::InvalidAdaptive(format!(
                "truncated-grid refinement exceeded {MAX_REFINED_POINTS} points; raise max_tiles or lower max_depth"
            )));
        }

        let origin = Vec3::from_array(tile.spec.origin);
        let mut values = Vec::with_capacity(count);
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in 0..dims[0] {
                    let point = origin + Vec3::new(x as f32, y as f32, z as f32) * finest;
                    values.push(local_graph.evaluate(point));
                }
            }
        }
        tiles.push(TileValues {
            spec: TileSpec {
                origin: tile.spec.origin,
                cell: finest,
                dims,
            },
            values,
        });
    }

    let refined = AdaptiveGrid {
        reference: grid.reference,
        tiles,
        levels: grid.levels.clone(),
        truncated: false,
    };
    Ok(refined)
}

pub fn mesh_adaptive_refined(graph: &FieldGraph, grid: &AdaptiveGrid) -> Result<Mesh, GpuError> {
    let refined = refine_adaptive_grid(graph, grid)?;
    let mut mesh = mesh_adaptive(&refined)?;
    orient_by_gradient(graph, &mut mesh);
    Ok(mesh)
}

pub fn mesh_adaptive_refined_dc(graph: &FieldGraph, grid: &AdaptiveGrid) -> Result<Mesh, GpuError> {
    let refined = refine_adaptive_grid(graph, grid)?;
    let mut mesh = mesh_adaptive_dc(graph, &refined)?;
    orient_by_gradient(graph, &mut mesh);
    Ok(mesh)
}

/// Refined SN/DC can produce locally inconsistent winding on CSG shapes even
/// though the surface is closed. Flip each triangle against the field gradient
/// and rebuild normals so volume and shading stay consistent.
pub(crate) fn orient_by_gradient(graph: &FieldGraph, mesh: &mut Mesh) {
    if mesh.indices.is_empty() {
        return;
    }
    for triangle in mesh.indices.chunks_exact_mut(3) {
        let a = Vec3::from_array(mesh.positions[triangle[0] as usize]);
        let b = Vec3::from_array(mesh.positions[triangle[1] as usize]);
        let c = Vec3::from_array(mesh.positions[triangle[2] as usize]);
        let face = (b - a).cross(c - a);
        if face.dot(graph.gradient((a + b + c) / 3.0)) < 0.0 {
            triangle.swap(1, 2);
        }
    }
    let mut normals = vec![Vec3::ZERO; mesh.positions.len()];
    for triangle in mesh.indices.chunks_exact(3) {
        let a = Vec3::from_array(mesh.positions[triangle[0] as usize]);
        let b = Vec3::from_array(mesh.positions[triangle[1] as usize]);
        let c = Vec3::from_array(mesh.positions[triangle[2] as usize]);
        let face = (b - a).cross(c - a);
        for vertex in triangle {
            normals[*vertex as usize] += face;
        }
    }
    mesh.normals = normals
        .into_iter()
        .map(|normal| {
            let normalized = normal.normalize_or_zero();
            if normalized.length_squared() < 0.5 {
                Vec3::Y
            } else {
                normalized
            }
            .to_array()
        })
        .collect();
}

#[allow(clippy::too_many_arguments)]
fn emit_edge(
    value_start: f32,
    value_end: f32,
    start: Key,
    axis: u8,
    cell_vertices: &HashMap<Key, u32>,
    edges_seen: &mut HashSet<(i32, i32, i32, u8)>,
    indices: &mut Vec<u32>,
    missing_quads: &mut usize,
) {
    if (value_start < 0.0) == (value_end < 0.0) {
        return;
    }
    let edge_key = (start[0], start[1], start[2], axis);
    if !edges_seen.insert(edge_key) {
        return;
    }

    let cells = match axis {
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

    let mut quad = [0u32; 4];
    for (index, cell) in cells.iter().enumerate() {
        match cell_vertices.get(cell) {
            Some(vertex) => quad[index] = *vertex,
            None => {
                *missing_quads += 1;
                return;
            }
        }
    }
    if value_start > 0.0 {
        quad.reverse();
    }
    indices.extend_from_slice(&[quad[0], quad[1], quad[2], quad[0], quad[2], quad[3]]);
}

fn lattice_position(origin: Vec3, cell: f32, cell_key: Key, corner: (i32, i32, i32)) -> Vec3 {
    origin
        + Vec3::new(
            (cell_key[0] + corner.0) as f32,
            (cell_key[1] + corner.1) as f32,
            (cell_key[2] + corner.2) as f32,
        ) * cell
}
