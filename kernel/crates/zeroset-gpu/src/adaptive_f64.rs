//! f64 sparse Surface Nets over a uniform adaptive leaf grid.
//!
//! The topology is identical to `adaptive_mesh::mesh_adaptive`: tiles are
//! placed on one global lattice and cell/edge keys are shared across tile
//! boundaries. Field values are re-evaluated in f64 at the lattice corners so
//! the vertex positions and crossings retain reference precision.

use std::collections::{HashMap, HashSet};

use glam::{DMat3, DVec3, Vec3};

use zeroset_core::{FieldGraph, MeshF64};

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

/// Mesh a uniform-cell adaptive grid with f64 values and positions.
pub fn mesh_adaptive_f64(graph: &FieldGraph, grid: &AdaptiveGrid) -> Result<MeshF64, GpuError> {
    if grid.tiles.is_empty() {
        return Ok(MeshF64::default());
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
                "mesh_adaptive_f64 requires uniform leaf cell sizes; the adaptive grid was truncated"
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
    let reference = grid.reference;
    let origin_f64 = DVec3::new(
        (reference.x + origin.x) as f64,
        (reference.y + origin.y) as f64,
        (reference.z + origin.z) as f64,
    );

    let mut corners: HashMap<Key, f64> = HashMap::new();
    let mut cell_vertices: HashMap<Key, u32> = HashMap::new();
    let mut positions: Vec<DVec3> = Vec::new();

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
                        .or_insert_with(|| {
                            graph.evaluate_f64(lattice_position_f64(
                                origin_f64,
                                cell as f64,
                                [base[0] + x, base[1] + y, base[2] + z],
                                (0, 0, 0),
                            ))
                        });
                }
            }
        }
    }

    for tile in &grid.tiles {
        let base = ((Vec3::from_array(tile.spec.origin) - origin) / cell).round();
        let base = [base.x as i32, base.y as i32, base.z as i32];
        let [nx, ny, nz] = tile.spec.dims;
        let (nx, ny, nz) = (nx as i32, ny as i32, nz as i32);
        for z in 0..nz - 1 {
            for y in 0..ny - 1 {
                for x in 0..nx - 1 {
                    let cell_key = [base[0] + x, base[1] + y, base[2] + z];
                    if cell_vertices.contains_key(&cell_key) {
                        continue;
                    }
                    let mut values = [0.0f64; 8];
                    let mut negative = 0u8;
                    for (index, (dx, dy, dz)) in CORNERS.iter().enumerate() {
                        let value = *corners
                            .get(&[cell_key[0] + dx, cell_key[1] + dy, cell_key[2] + dz])
                            .ok_or_else(|| {
                                GpuError::InvalidAdaptive(
                                    "adaptive f64 mesh corner is missing".to_string(),
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

                    let mut sum = DVec3::ZERO;
                    let mut crossings = 0u32;
                    for (a, b) in EDGES {
                        let va = values[a];
                        let vb = values[b];
                        if (va < 0.0) == (vb < 0.0) {
                            continue;
                        }
                        let denominator = va - vb;
                        let t = if denominator.abs() > f64::EPSILON {
                            (va / denominator).clamp(0.0, 1.0)
                        } else {
                            0.5
                        };
                        let pa =
                            lattice_position_f64(origin_f64, cell as f64, cell_key, CORNERS[a]);
                        let pb =
                            lattice_position_f64(origin_f64, cell as f64, cell_key, CORNERS[b]);
                        sum += pa.lerp(pb, t);
                        crossings += 1;
                    }
                    if crossings == 0 {
                        continue;
                    }
                    let index = positions.len() as u32;
                    positions.push(sum / crossings as f64);
                    cell_vertices.insert(cell_key, index);
                }
            }
        }
    }

    let mut indices = Vec::new();
    let mut edges_seen: HashSet<(i32, i32, i32, u8)> = HashSet::new();
    let mut missing_quads = 0usize;

    for cell_key in cell_vertices.keys() {
        let [cx, cy, cz] = *cell_key;
        let mut cell_values = [0.0f64; 8];
        for (index, (dx, dy, dz)) in CORNERS.iter().enumerate() {
            cell_values[index] = *corners.get(&[cx + dx, cy + dy, cz + dz]).ok_or_else(|| {
                GpuError::InvalidAdaptive("adaptive f64 mesh corner is missing".to_string())
            })?;
        }
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
        return Ok(MeshF64::default());
    }

    let mut volume = 0.0f64;
    for triangle in indices.chunks_exact(3) {
        let a = positions[triangle[0] as usize];
        let b = positions[triangle[1] as usize];
        let c = positions[triangle[2] as usize];
        volume += a.dot(b.cross(c)) / 6.0;
    }
    if volume < 0.0 {
        for triangle in indices.chunks_exact_mut(3) {
            triangle.swap(1, 2);
        }
    }

    let mut normals = vec![DVec3::ZERO; positions.len()];
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
            *normal = DVec3::Y;
        }
    }

    let _ = missing_quads;
    Ok(MeshF64 {
        positions: positions
            .iter()
            .map(|position| position.to_array())
            .collect(),
        normals: normals.iter().map(|normal| normal.to_array()).collect(),
        indices,
    })
}

/// Refine mixed-level leaves to the finest cell, then run f64 Surface Nets.
pub fn mesh_adaptive_refined_f64(
    graph: &FieldGraph,
    grid: &AdaptiveGrid,
) -> Result<MeshF64, GpuError> {
    let refined = crate::refine_adaptive_grid(graph, grid)?;
    mesh_adaptive_f64(graph, &refined)
}

/// f64 Dual Contouring over the same sparse topology as `mesh_adaptive_f64`.
pub fn mesh_adaptive_f64_dc(graph: &FieldGraph, grid: &AdaptiveGrid) -> Result<MeshF64, GpuError> {
    let mut mesh = mesh_adaptive_f64(graph, grid)?;
    if mesh.positions.is_empty() || grid.tiles.is_empty() {
        return Ok(mesh);
    }
    let cell = grid.tiles[0].spec.cell;
    if !cell.is_finite() || cell <= 0.0 {
        return Err(GpuError::InvalidAdaptive(
            "adaptive tiles have an invalid cell size".to_string(),
        ));
    }
    let origin = grid
        .tiles
        .iter()
        .map(|tile| Vec3::from_array(tile.spec.origin))
        .reduce(Vec3::min)
        .expect("tiles is non-empty");
    let reference = grid.reference;
    let origin_f64 = DVec3::new(
        (reference.x + origin.x) as f64,
        (reference.y + origin.y) as f64,
        (reference.z + origin.z) as f64,
    );

    let mut corners: HashMap<Key, f64> = HashMap::new();
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
                        .or_insert_with(|| {
                            graph.evaluate_f64(lattice_position_f64(
                                origin_f64,
                                cell as f64,
                                [base[0] + x, base[1] + y, base[2] + z],
                                (0, 0, 0),
                            ))
                        });
                }
            }
        }
    }

    let mut seen = HashSet::new();
    let mut positions = Vec::with_capacity(mesh.positions.len());
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
                    let mut values = [0.0f64; 8];
                    let mut negative = 0u8;
                    for (index, (dx, dy, dz)) in CORNERS.iter().enumerate() {
                        let value = *corners
                            .get(&[cell_key[0] + dx, cell_key[1] + dy, cell_key[2] + dz])
                            .ok_or_else(|| {
                                GpuError::InvalidAdaptive(
                                    "adaptive f64 DC corner is missing".to_string(),
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

                    let mut ata = DMat3::ZERO;
                    let mut atb = DVec3::ZERO;
                    let mut centroid = DVec3::ZERO;
                    let mut crossings = 0u32;
                    let mut planes: Vec<(DVec3, f64)> = Vec::new();
                    for (a, b) in EDGES {
                        let va = values[a];
                        let vb = values[b];
                        if (va < 0.0) == (vb < 0.0) {
                            continue;
                        }
                        let denominator = va - vb;
                        let t = if denominator.abs() > f64::EPSILON {
                            (va / denominator).clamp(0.0, 1.0)
                        } else {
                            0.5
                        };
                        let pa =
                            lattice_position_f64(origin_f64, cell as f64, cell_key, CORNERS[a]);
                        let pb =
                            lattice_position_f64(origin_f64, cell as f64, cell_key, CORNERS[b]);
                        let intersection = pa.lerp(pb, t);
                        centroid += intersection;
                        crossings += 1;
                        let normal = graph.gradient_f64(intersection);
                        if normal.length_squared() > 1e-24 {
                            let normal = normal.normalize();
                            planes.push((normal, normal.dot(intersection)));
                            ata += DMat3::from_cols(
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
                    let mut unique: Vec<(DVec3, f64)> = Vec::new();
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
                            if determinant.abs() < 1e-16 {
                                continue;
                            }
                            let a = (d1 - cosine * d2) / determinant;
                            let b = (d2 - cosine * d1) / determinant;
                            let point = n1 * a + n2 * b;
                            let direction = n1.cross(n2).normalize_or_zero();
                            if direction.length_squared() < 0.5 {
                                continue;
                            }
                            let projector = DMat3::IDENTITY
                                - DMat3::from_cols(
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
                            let normals = DMat3::from_cols(n1, n2, n3);
                            let determinant = normals.determinant();
                            if determinant.abs() > 1e-16 {
                                let corner = normals.inverse() * DVec3::new(d1, d2, d3);
                                if corner.is_finite() {
                                    let weight = 2.0
                                        + 4.0 * (1.0 - n1.dot(n2).max(n1.dot(n3)).max(n2.dot(n3)));
                                    ata += DMat3::IDENTITY * weight;
                                    atb += corner * weight;
                                }
                            }
                        }
                    }
                    centroid /= crossings as f64;
                    let lambda = if unique.len() >= 2 && unique[0].0.dot(unique[1].0) < 0.5 {
                        1e-4
                    } else {
                        1e-3
                    };
                    ata += DMat3::IDENTITY * lambda;
                    atb += centroid * lambda;
                    let determinant = ata.determinant();
                    let mut vertex = if determinant.abs() > 1e-16 {
                        ata.inverse() * atb
                    } else {
                        centroid
                    };
                    if !vertex.is_finite() {
                        vertex = centroid;
                    }
                    let cell_min = origin_f64
                        + DVec3::new(cell_key[0] as f64, cell_key[1] as f64, cell_key[2] as f64)
                            * cell as f64;
                    vertex = vertex.clamp(cell_min, cell_min + DVec3::splat(cell as f64));
                    positions.push(vertex);
                }
            }
        }
    }

    if positions.len() != mesh.positions.len() {
        return Err(GpuError::InvalidAdaptive(format!(
            "adaptive f64 DC vertex count mismatch: {} vs {}",
            positions.len(),
            mesh.positions.len()
        )));
    }
    for (index, position) in positions.iter().enumerate() {
        mesh.positions[index] = position.to_array();
    }

    let mut normals = vec![DVec3::ZERO; mesh.positions.len()];
    for triangle in mesh.indices.chunks_exact(3) {
        let a = DVec3::from_array(mesh.positions[triangle[0] as usize]);
        let b = DVec3::from_array(mesh.positions[triangle[1] as usize]);
        let c = DVec3::from_array(mesh.positions[triangle[2] as usize]);
        let face = (b - a).cross(c - a);
        normals[triangle[0] as usize] += face;
        normals[triangle[1] as usize] += face;
        normals[triangle[2] as usize] += face;
    }
    for normal in &mut normals {
        *normal = normal.normalize_or_zero();
        if normal.length_squared() < 0.5 {
            *normal = DVec3::Y;
        }
    }
    mesh.normals = normals.iter().map(|normal| normal.to_array()).collect();
    Ok(mesh)
}

/// Refine mixed-level leaves, then run f64 Dual Contouring.
pub fn mesh_adaptive_refined_dc_f64(
    graph: &FieldGraph,
    grid: &AdaptiveGrid,
) -> Result<MeshF64, GpuError> {
    let refined = crate::refine_adaptive_grid(graph, grid)?;
    mesh_adaptive_f64_dc(graph, &refined)
}

#[allow(clippy::too_many_arguments)]
fn emit_edge(
    value_start: f64,
    value_end: f64,
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
    if !edges_seen.insert((start[0], start[1], start[2], axis)) {
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

fn lattice_position_f64(origin: DVec3, cell: f64, cell_key: Key, corner: (i32, i32, i32)) -> DVec3 {
    origin
        + DVec3::new(
            (cell_key[0] + corner.0) as f64,
            (cell_key[1] + corner.1) as f64,
            (cell_key[2] + corner.2) as f64,
        ) * cell
}
