use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use glam::{DMat3, DVec3, Mat3, Vec3};

use crate::{Aabb, FieldGraph, KernelError, NodeId};

/// Dense regular grid of field samples.
///
/// `dims` counts *corner samples* per axis, so the number of cells is
/// `dims - 1`. Values are stored x-fastest:
/// `index = x + dims.x * (y + dims.y * z)`.
#[derive(Clone, Debug)]
pub struct GridSamples {
    pub bounds: Aabb,
    pub cell: f32,
    pub dims: [usize; 3],
    pub values: Vec<f32>,
}

impl GridSamples {
    #[inline]
    pub fn corner_count(&self) -> usize {
        self.dims[0] * self.dims[1] * self.dims[2]
    }

    #[inline]
    pub fn index(&self, x: usize, y: usize, z: usize) -> usize {
        x + self.dims[0] * (y + self.dims[1] * z)
    }

    #[inline]
    pub fn value(&self, x: usize, y: usize, z: usize) -> f32 {
        self.values[self.index(x, y, z)]
    }

    #[inline]
    pub fn position(&self, x: usize, y: usize, z: usize) -> Vec3 {
        self.bounds.min + Vec3::new(x as f32, y as f32, z as f32) * self.cell
    }

    /// Extract an iso-surface at `value == 0` with Surface Nets.
    pub fn surface_nets(&self) -> Result<Mesh, KernelError> {
        surface_nets(self)
    }

    /// Extract an iso-surface with Dual Contouring and a QEF per cell.
    ///
    /// The fallback gradient is the trilinear gradient of the sampled field.
    /// Pass the graph's analytic gradient via [`Self::dual_contour_with_gradient`]
    /// for sharper feature preservation.
    pub fn dual_contour(&self) -> Result<Mesh, KernelError> {
        self.dual_contour_with_gradient(|point| self.trilinear_gradient(point))
    }

    /// Dual Contouring with a caller-provided gradient function.
    pub fn dual_contour_with_gradient<F>(&self, gradient: F) -> Result<Mesh, KernelError>
    where
        F: Fn(Vec3) -> Vec3,
    {
        dual_contour(self, &gradient)
    }

    fn trilinear_gradient(&self, point: Vec3) -> Vec3 {
        let h = self.cell * 0.25;
        let dx = Vec3::X * h;
        let dy = Vec3::Y * h;
        let dz = Vec3::Z * h;
        let denominator = 2.0 * h;
        Vec3::new(
            (self.sample_trilinear(point + dx) - self.sample_trilinear(point - dx)) / denominator,
            (self.sample_trilinear(point + dy) - self.sample_trilinear(point - dy)) / denominator,
            (self.sample_trilinear(point + dz) - self.sample_trilinear(point - dz)) / denominator,
        )
    }

    fn sample_trilinear(&self, point: Vec3) -> f32 {
        let local = (point - self.bounds.min) / self.cell;
        let max = Vec3::new(
            self.dims[0] as f32 - 1.0,
            self.dims[1] as f32 - 1.0,
            self.dims[2] as f32 - 1.0,
        );
        let local = local.clamp(Vec3::ZERO, max - Vec3::splat(1e-4));
        let x0 = local.x.floor() as usize;
        let y0 = local.y.floor() as usize;
        let z0 = local.z.floor() as usize;
        let x1 = (x0 + 1).min(self.dims[0] - 1);
        let y1 = (y0 + 1).min(self.dims[1] - 1);
        let z1 = (z0 + 1).min(self.dims[2] - 1);
        let tx = local.x - x0 as f32;
        let ty = local.y - y0 as f32;
        let tz = local.z - z0 as f32;
        let c00 = self.value(x0, y0, z0) * (1.0 - tx) + self.value(x1, y0, z0) * tx;
        let c10 = self.value(x0, y1, z0) * (1.0 - tx) + self.value(x1, y1, z0) * tx;
        let c01 = self.value(x0, y0, z1) * (1.0 - tx) + self.value(x1, y0, z1) * tx;
        let c11 = self.value(x0, y1, z1) * (1.0 - tx) + self.value(x1, y1, z1) * tx;
        let c0 = c00 * (1.0 - ty) + c10 * ty;
        let c1 = c01 * (1.0 - ty) + c11 * ty;
        c0 * (1.0 - tz) + c1 * tz
    }
}

impl FieldGraph {
    /// Sample a dense grid over the field bounds, padded by two cells so a
    /// closed surface is not clipped at the grid boundary.
    ///
    /// `resolution` is the number of cells along the longest axis.
    pub fn sample_grid(&self, resolution: u32) -> Result<GridSamples, KernelError> {
        let root = self.root().ok_or(KernelError::InvalidBounds)?;
        self.sample_grid_at(root, resolution)
    }

    /// Sample the root at `resolution` and extract with Dual Contouring.
    pub fn dual_contour(&self, resolution: u32) -> Result<Mesh, KernelError> {
        let grid = self.sample_grid(resolution)?;
        grid.dual_contour_with_gradient(|point| self.gradient(point))
    }

    /// Sample a dense grid for a specific node, not only the graph root.
    pub fn sample_grid_at(
        &self,
        node: NodeId,
        resolution: u32,
    ) -> Result<GridSamples, KernelError> {
        if !(2..=1024).contains(&resolution) {
            return Err(KernelError::InvalidResolution(resolution));
        }

        let bounds = self.bounds_of(node);
        if bounds.is_empty() || !bounds.is_finite() {
            return Err(KernelError::InvalidBounds);
        }

        let max_extent = bounds.max_extent();
        if max_extent <= 0.0 || !max_extent.is_finite() {
            return Err(KernelError::InvalidBounds);
        }

        let cell = max_extent / resolution as f32;
        let pad = cell * 2.0;
        let min = bounds.min - Vec3::splat(pad);
        let max = bounds.max + Vec3::splat(pad);

        let nx = (((max.x - min.x) / cell).ceil() as usize) + 1;
        let ny = (((max.y - min.y) / cell).ceil() as usize) + 1;
        let nz = (((max.z - min.z) / cell).ceil() as usize) + 1;
        let corners = nx
            .checked_mul(ny)
            .and_then(|n| n.checked_mul(nz))
            .ok_or(KernelError::GridTooLarge)?;

        if corners > 1_000_000_000 {
            return Err(KernelError::GridTooLarge);
        }

        let mut values = Vec::with_capacity(corners);
        for z in 0..nz {
            for y in 0..ny {
                for x in 0..nx {
                    let point = min + Vec3::new(x as f32, y as f32, z as f32) * cell;
                    values.push(self.evaluate_at(node, point));
                }
            }
        }

        Ok(GridSamples {
            bounds: Aabb { min, max },
            cell,
            dims: [nx, ny, nz],
            values,
        })
    }
}

/// Dense f64 grid used as a precision reference path.
#[derive(Clone, Debug)]
pub struct GridSamplesF64 {
    pub min: DVec3,
    pub cell: f64,
    pub dims: [usize; 3],
    pub values: Vec<f64>,
}

impl GridSamplesF64 {
    #[inline]
    pub fn index(&self, x: usize, y: usize, z: usize) -> usize {
        x + self.dims[0] * (y + self.dims[1] * z)
    }

    #[inline]
    pub fn value(&self, x: usize, y: usize, z: usize) -> f64 {
        self.values[self.index(x, y, z)]
    }

    #[inline]
    pub fn position(&self, x: usize, y: usize, z: usize) -> DVec3 {
        self.min + DVec3::new(x as f64, y as f64, z as f64) * self.cell
    }

    /// Surface Nets with f64 crossings and vertex positions, emitted in a local
    /// f32 frame around `reference`. This keeps a far-away model precise while
    /// still producing the kernel's normal `Mesh`.
    pub fn surface_nets_local(&self, reference: DVec3) -> Result<Mesh, KernelError> {
        surface_nets_f64(self, reference)
    }

    /// f64 Surface Nets in world coordinates (no f32 local conversion).
    pub fn surface_nets_world(&self) -> Result<MeshF64, KernelError> {
        surface_nets_f64_world(self)
    }

    /// Dual Contouring with f64 crossings/QEF, emitted in a local f32 frame.
    pub fn dual_contour_local(&self, reference: DVec3) -> Result<Mesh, KernelError> {
        self.dual_contour_local_with_gradient(reference, |point| self.trilinear_gradient(point))
    }

    /// f64 Dual Contouring with a caller-provided f64 gradient.
    pub fn dual_contour_local_with_gradient<F>(
        &self,
        reference: DVec3,
        gradient: F,
    ) -> Result<Mesh, KernelError>
    where
        F: Fn(DVec3) -> DVec3,
    {
        dual_contour_f64(self, reference, &gradient)
    }

    /// f64 Dual Contouring in world coordinates.
    pub fn dual_contour_world(&self) -> Result<MeshF64, KernelError> {
        self.dual_contour_world_with_gradient(|point| self.trilinear_gradient(point))
    }

    /// f64 world-space Dual Contouring with a caller-provided gradient.
    pub fn dual_contour_world_with_gradient<F>(&self, gradient: F) -> Result<MeshF64, KernelError>
    where
        F: Fn(DVec3) -> DVec3,
    {
        dual_contour_f64_world(self, &gradient)
    }

    fn sample_trilinear(&self, point: DVec3) -> f64 {
        let local = (point - self.min) / self.cell;
        let max = DVec3::new(
            self.dims[0] as f64 - 1.0,
            self.dims[1] as f64 - 1.0,
            self.dims[2] as f64 - 1.0,
        );
        let local = local.clamp(DVec3::ZERO, max - DVec3::splat(1e-9));
        let x0 = local.x.floor() as usize;
        let y0 = local.y.floor() as usize;
        let z0 = local.z.floor() as usize;
        let x1 = (x0 + 1).min(self.dims[0] - 1);
        let y1 = (y0 + 1).min(self.dims[1] - 1);
        let z1 = (z0 + 1).min(self.dims[2] - 1);
        let tx = local.x - x0 as f64;
        let ty = local.y - y0 as f64;
        let tz = local.z - z0 as f64;
        let c00 = self.value(x0, y0, z0) * (1.0 - tx) + self.value(x1, y0, z0) * tx;
        let c10 = self.value(x0, y1, z0) * (1.0 - tx) + self.value(x1, y1, z0) * tx;
        let c01 = self.value(x0, y0, z1) * (1.0 - tx) + self.value(x1, y0, z1) * tx;
        let c11 = self.value(x0, y1, z1) * (1.0 - tx) + self.value(x1, y1, z1) * tx;
        let c0 = c00 * (1.0 - ty) + c10 * ty;
        let c1 = c01 * (1.0 - ty) + c11 * ty;
        c0 * (1.0 - tz) + c1 * tz
    }

    fn trilinear_gradient(&self, point: DVec3) -> DVec3 {
        let h = self.cell * 0.25;
        let dx = DVec3::X * h;
        let dy = DVec3::Y * h;
        let dz = DVec3::Z * h;
        let denominator = 2.0 * h;
        DVec3::new(
            (self.sample_trilinear(point + dx) - self.sample_trilinear(point - dx)) / denominator,
            (self.sample_trilinear(point + dy) - self.sample_trilinear(point - dy)) / denominator,
            (self.sample_trilinear(point + dz) - self.sample_trilinear(point - dz)) / denominator,
        )
    }
}

impl FieldGraph {
    /// Sample an f64 grid over the field bounds. Values and crossings use f64;
    /// the resulting mesh is emitted in a local frame around `reference`.
    pub fn sample_grid_f64(&self, resolution: u32) -> Result<GridSamplesF64, KernelError> {
        let root = self.root().ok_or(KernelError::InvalidBounds)?;
        if !(2..=1024).contains(&resolution) {
            return Err(KernelError::InvalidResolution(resolution));
        }
        let bounds = self.bounds_of(root);
        if bounds.is_empty() || !bounds.is_finite() {
            return Err(KernelError::InvalidBounds);
        }
        let max_extent = bounds.max_extent();
        if max_extent <= 0.0 || !max_extent.is_finite() {
            return Err(KernelError::InvalidBounds);
        }

        let cell = max_extent as f64 / resolution as f64;
        let pad = cell * 2.0;
        let min = DVec3::new(
            bounds.min.x as f64,
            bounds.min.y as f64,
            bounds.min.z as f64,
        ) - DVec3::splat(pad);
        let max = DVec3::new(
            bounds.max.x as f64,
            bounds.max.y as f64,
            bounds.max.z as f64,
        ) + DVec3::splat(pad);
        let nx = (((max.x - min.x) / cell).ceil() as usize) + 1;
        let ny = (((max.y - min.y) / cell).ceil() as usize) + 1;
        let nz = (((max.z - min.z) / cell).ceil() as usize) + 1;
        let corners = nx
            .checked_mul(ny)
            .and_then(|count| count.checked_mul(nz))
            .ok_or(KernelError::GridTooLarge)?;
        if corners > 1_000_000_000 {
            return Err(KernelError::GridTooLarge);
        }

        let mut values = Vec::with_capacity(corners);
        for z in 0..nz {
            for y in 0..ny {
                for x in 0..nx {
                    let point = min + DVec3::new(x as f64, y as f64, z as f64) * cell;
                    values.push(self.evaluate_f64(point));
                }
            }
        }
        Ok(GridSamplesF64 {
            min,
            cell,
            dims: [nx, ny, nz],
            values,
        })
    }
}

/// A triangle mesh produced by the kernel.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    /// Vertex positions, x/y/z interleaved per vertex in the array element.
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

/// f64 mesh for precision-reference paths.
#[derive(Clone, Debug, Default)]
pub struct MeshF64 {
    pub positions: Vec<[f64; 3]>,
    pub normals: Vec<[f64; 3]>,
    pub indices: Vec<u32>,
}

impl MeshF64 {
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Convert to the kernel's f32 mesh in a local frame around `reference`.
    pub fn to_f32_local(&self, reference: DVec3) -> Mesh {
        Mesh {
            positions: self
                .positions
                .iter()
                .map(|p| (DVec3::from_array(*p) - reference).as_vec3().to_array())
                .collect(),
            normals: self
                .normals
                .iter()
                .map(|n| DVec3::from_array(*n).as_vec3().to_array())
                .collect(),
            indices: self.indices.clone(),
        }
    }

    /// Convert to a world-space f32 mesh (STL and legacy consumers are f32).
    pub fn to_f32_world(&self) -> Mesh {
        Mesh {
            positions: self
                .positions
                .iter()
                .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
                .collect(),
            normals: self
                .normals
                .iter()
                .map(|n| [n[0] as f32, n[1] as f32, n[2] as f32])
                .collect(),
            indices: self.indices.clone(),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Quadric {
    a: [[f64; 3]; 3],
    b: [f64; 3],
    c: f64,
}

impl Quadric {
    fn from_plane(normal: Vec3, offset: f64) -> Self {
        let n = [normal.x as f64, normal.y as f64, normal.z as f64];
        let mut a = [[0.0; 3]; 3];
        for row in 0..3 {
            for column in 0..3 {
                a[row][column] = n[row] * n[column];
            }
        }
        Self {
            a,
            b: [n[0] * offset, n[1] * offset, n[2] * offset],
            c: offset * offset,
        }
    }

    fn add(self, other: Self) -> Self {
        let mut result = self;
        for row in 0..3 {
            for column in 0..3 {
                result.a[row][column] += other.a[row][column];
            }
            result.b[row] += other.b[row];
        }
        result.c += other.c;
        result
    }

    fn evaluate(&self, point: Vec3) -> f64 {
        let p = [point.x as f64, point.y as f64, point.z as f64];
        let mut value = self.c;
        for row in 0..3 {
            for column in 0..3 {
                value += p[row] * self.a[row][column] * p[column];
            }
            value += 2.0 * self.b[row] * p[row];
        }
        value.max(0.0)
    }

    fn optimal(&self, midpoint: Vec3, extent: f32) -> Vec3 {
        let a = DMat3::from_cols(
            DVec3::new(self.a[0][0], self.a[1][0], self.a[2][0]),
            DVec3::new(self.a[0][1], self.a[1][1], self.a[2][1]),
            DVec3::new(self.a[0][2], self.a[1][2], self.a[2][2]),
        );
        let b = DVec3::new(self.b[0], self.b[1], self.b[2]);
        let determinant = a.determinant();
        let center = midpoint.as_dvec3();
        let limit = DVec3::splat((extent.max(1e-5) * 1.5) as f64);
        if determinant.abs() > 1e-12 {
            let candidate = a.inverse() * (-b);
            if candidate.is_finite() {
                return candidate.clamp(center - limit, center + limit).as_vec3();
            }
        }
        midpoint
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoundaryCapReport {
    pub boundary_edges: usize,
    pub non_manifold_edges: usize,
    pub branch_vertices: usize,
    pub loops: usize,
    pub skipped_edges: usize,
}

impl Mesh {
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Cluster vertices into a uniform grid and drop degenerate triangles.
    ///
    /// `ratio` is the target fraction of triangles to retain. This is a fast
    /// preview-grade simplifier; it does not preserve manifoldness or sharp
    /// features. Use the original mesh when closedness matters.
    pub fn simplify_clustered(&self, ratio: f32) -> Mesh {
        if self.indices.is_empty() || ratio >= 0.999 {
            return self.clone();
        }
        let ratio = ratio.clamp(0.02, 1.0);
        let target = ((self.triangle_count() as f32) * ratio).max(1.0) as usize;
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for position in &self.positions {
            let position = Vec3::from_array(*position);
            min = min.min(position);
            max = max.max(position);
        }
        let diagonal = (max - min).length().max(1e-6);
        let mut low = 0.0f32;
        let mut high = diagonal;
        let mut best = self.clone();
        for _ in 0..14 {
            let cell = (low + high) * 0.5;
            if cell <= 1e-7 {
                break;
            }
            let candidate = self.cluster_vertices(cell);
            let triangles = candidate.triangle_count();
            if triangles > target {
                low = cell;
            } else {
                high = cell;
                best = candidate;
                if triangles + target / 20 >= target {
                    break;
                }
            }
        }
        best
    }

    fn cluster_vertices(&self, cell: f32) -> Mesh {
        let cell = cell.max(1e-7);
        let mut clusters: std::collections::HashMap<(i32, i32, i32), (Vec3, Vec3, u32)> =
            std::collections::HashMap::new();
        let mut remap = vec![0u32; self.positions.len()];
        for (index, position) in self.positions.iter().enumerate() {
            let point = Vec3::from_array(*position);
            let key = (
                (point.x / cell).floor() as i32,
                (point.y / cell).floor() as i32,
                (point.z / cell).floor() as i32,
            );
            let normal = self
                .normals
                .get(index)
                .map(|normal| Vec3::from_array(*normal))
                .unwrap_or(Vec3::Y);
            let entry = clusters.entry(key).or_insert((Vec3::ZERO, Vec3::ZERO, 0));
            entry.0 += point;
            entry.1 += normal;
            entry.2 += 1;
        }

        // Build stable cluster indices from map entries, then remap by key.
        let mut cluster_index = std::collections::HashMap::new();
        let mut positions = Vec::with_capacity(clusters.len());
        let mut normals = Vec::with_capacity(clusters.len());
        for (key, (sum, normal_sum, count)) in &clusters {
            let index = positions.len() as u32;
            cluster_index.insert(*key, index);
            positions.push((*sum / *count as f32).to_array());
            let normal = if normal_sum.length_squared() > 1e-12 {
                normal_sum.normalize()
            } else {
                Vec3::Y
            };
            normals.push(normal.to_array());
        }
        for (index, position) in self.positions.iter().enumerate() {
            let point = Vec3::from_array(*position);
            let key = (
                (point.x / cell).floor() as i32,
                (point.y / cell).floor() as i32,
                (point.z / cell).floor() as i32,
            );
            remap[index] = cluster_index[&key];
        }

        let mut indices = Vec::with_capacity(self.indices.len());
        for triangle in self.indices.chunks_exact(3) {
            let a = remap[triangle[0] as usize];
            let b = remap[triangle[1] as usize];
            let c = remap[triangle[2] as usize];
            if a == b || b == c || a == c {
                continue;
            }
            indices.extend_from_slice(&[a, b, c]);
        }
        Mesh {
            positions,
            normals,
            indices,
        }
    }

    /// Edge-collapse simplification with a link-condition check and QEM cost.
    ///
    /// Edges are collapsed by quadric error while preserving manifold topology
    /// for closed meshes. The optimal vertex is clamped near the edge midpoint.
    pub fn simplify_edge_collapse(&self, ratio: f32) -> Mesh {
        self.simplify_edge_collapse_with_angle(ratio, 30.0)
    }

    /// Edge-collapse simplification with an explicit crease threshold.
    pub fn simplify_edge_collapse_with_angle(&self, ratio: f32, crease_degrees: f32) -> Mesh {
        if self.indices.is_empty() || ratio >= 0.999 {
            return self.clone();
        }
        let target = ((self.triangle_count() as f32) * ratio.clamp(0.02, 1.0)).max(1.0) as usize;
        let mut alive = vec![true; self.positions.len()];
        let mut positions: Vec<Vec3> = self
            .positions
            .iter()
            .map(|position| Vec3::from_array(*position))
            .collect();
        let mut normals: Vec<Vec3> = self
            .normals
            .iter()
            .map(|normal| Vec3::from_array(*normal))
            .collect();
        if normals.len() < positions.len() {
            normals.resize(positions.len(), Vec3::Y);
        }
        let mut neighbors: Vec<std::collections::HashSet<u32>> =
            vec![std::collections::HashSet::new(); positions.len()];
        let mut triangles: Vec<Option<[u32; 3]>> = Vec::with_capacity(self.indices.len() / 3);
        for triangle in self.indices.chunks_exact(3) {
            let tri = [triangle[0], triangle[1], triangle[2]];
            for (index, vertex) in tri.iter().enumerate() {
                neighbors[*vertex as usize].insert(tri[(index + 1) % 3]);
                neighbors[*vertex as usize].insert(tri[(index + 2) % 3]);
            }
            triangles.push(Some(tri));
        }
        let mut edge_live: HashMap<(u32, u32), usize> = HashMap::new();
        for tri in triangles.iter().flatten() {
            for edge in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                let key = (edge.0.min(edge.1), edge.0.max(edge.1));
                *edge_live.entry(key).or_default() += 1;
            }
        }

        let mut quadrics = vec![Quadric::default(); positions.len()];
        let mut face_normals = vec![Vec3::Y; triangles.len()];
        let mut edge_faces: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
        for (face, tri) in triangles.iter().enumerate() {
            let Some(tri) = tri else {
                continue;
            };
            for edge in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                let key = (edge.0.min(edge.1), edge.0.max(edge.1));
                edge_faces.entry(key).or_default().push(face);
            }
            let a = positions[tri[0] as usize];
            let b = positions[tri[1] as usize];
            let c = positions[tri[2] as usize];
            let normal = (b - a).cross(c - a).normalize_or_zero();
            if normal.length_squared() < 0.5 {
                continue;
            }
            face_normals[face] = normal;
            let offset = -(normal.dot(a) as f64);
            let quadric = Quadric::from_plane(normal, offset);
            for vertex in tri {
                quadrics[*vertex as usize] = quadrics[*vertex as usize].add(quadric);
            }
        }

        // Perpendicular boundary planes keep open borders from drifting.
        for ((u, v), faces) in &edge_faces {
            if faces.len() != 1 {
                continue;
            }
            let a = positions[*u as usize];
            let b = positions[*v as usize];
            let edge = (b - a).normalize_or_zero();
            let outward = edge.cross(face_normals[faces[0]]).normalize_or_zero();
            if outward.length_squared() < 0.5 {
                continue;
            }
            let quadric = Quadric::from_plane(outward, -(outward.dot(a) as f64));
            quadrics[*u as usize] = quadrics[*u as usize].add(quadric);
            quadrics[*v as usize] = quadrics[*v as usize].add(quadric);
        }

        // Sharp edges get a bisector plane so collapsing vertices prefer the
        // crease line instead of sliding across either face.
        let sharp_threshold = crease_degrees.clamp(0.0, 180.0).to_radians().cos();
        for ((u, v), faces) in &edge_faces {
            if faces.len() != 2 {
                continue;
            }
            let n1 = face_normals[faces[0]];
            let n2 = face_normals[faces[1]];
            if n1.dot(n2) >= sharp_threshold {
                continue;
            }
            let bisector = (n1 + n2).normalize_or_zero();
            if bisector.length_squared() < 0.5 {
                continue;
            }
            let a = positions[*u as usize];
            let quadric = Quadric::from_plane(bisector, -(bisector.dot(a) as f64));
            quadrics[*u as usize] = quadrics[*u as usize].add(quadric).add(quadric);
            quadrics[*v as usize] = quadrics[*v as usize].add(quadric).add(quadric);
        }

        let mut feature_vertices = vec![false; positions.len()];
        let mut sharp_edges = std::collections::HashSet::new();
        for [u, v] in self.feature_edges(crease_degrees) {
            feature_vertices[u as usize] = true;
            feature_vertices[v as usize] = true;
            sharp_edges.insert((u.min(v), u.max(v)));
        }

        let mut heap = std::collections::BinaryHeap::new();
        for (vertex, adjacent) in neighbors.iter().enumerate() {
            for other in adjacent {
                if (vertex as u32) < *other {
                    let (u, v) = (vertex as u32, *other);
                    let midpoint = (positions[u as usize] + positions[v as usize]) * 0.5;
                    let extent = positions[u as usize].distance(positions[v as usize]);
                    let combined = quadrics[u as usize].add(quadrics[v as usize]);
                    let candidate = combined.optimal(midpoint, extent);
                    let cost = combined.evaluate(candidate);
                    heap.push(std::cmp::Reverse((cost.to_bits(), u, v)));
                }
            }
        }

        let mut live_triangles = triangles.len();
        while live_triangles > target {
            let Some(std::cmp::Reverse((_, mut u, mut v))) = heap.pop() else {
                break;
            };
            if !alive[u as usize] || !alive[v as usize] || !neighbors[u as usize].contains(&v) {
                continue;
            }
            if feature_vertices[v as usize] && !feature_vertices[u as usize] {
                std::mem::swap(&mut u, &mut v);
            }
            let common: Vec<u32> = neighbors[u as usize]
                .intersection(&neighbors[v as usize])
                .copied()
                .collect();
            let edge_key = (u.min(v), u.max(v));
            let is_sharp = sharp_edges.contains(&edge_key);
            if feature_vertices[u as usize] && feature_vertices[v as usize] && !is_sharp {
                continue;
            }
            let boundary = edge_live.get(&edge_key).copied().unwrap_or(0) == 1;
            if !((boundary && common.len() == 1) || (!boundary && common.len() == 2)) {
                continue;
            }
            let midpoint = (positions[u as usize] + positions[v as usize]) * 0.5;
            let extent = positions[u as usize].distance(positions[v as usize]);
            let combined = quadrics[u as usize].add(quadrics[v as usize]);
            positions[u as usize] = if feature_vertices[u as usize] && !feature_vertices[v as usize]
            {
                positions[u as usize]
            } else {
                combined.optimal(midpoint, extent)
            };
            feature_vertices[u as usize] |= feature_vertices[v as usize];
            feature_vertices[v as usize] = false;
            quadrics[u as usize] = combined;
            normals[u as usize] = (normals[u as usize] + normals[v as usize]).normalize_or_zero();
            if normals[u as usize].length_squared() < 0.5 {
                normals[u as usize] = Vec3::Y;
            }

            let moved: Vec<u32> = neighbors[v as usize].iter().copied().collect();
            for neighbor in moved {
                if neighbor == u {
                    continue;
                }
                neighbors[neighbor as usize].remove(&v);
                neighbors[neighbor as usize].insert(u);
                neighbors[u as usize].insert(neighbor);
            }
            neighbors[u as usize].remove(&v);
            neighbors[v as usize].clear();
            alive[v as usize] = false;

            for triangle in &mut triangles {
                let Some(tri) = triangle else {
                    continue;
                };
                if !tri.contains(&v) {
                    continue;
                }
                let old = *tri;
                for edge in [(old[0], old[1]), (old[1], old[2]), (old[2], old[0])] {
                    let key = (edge.0.min(edge.1), edge.0.max(edge.1));
                    *edge_live.entry(key).or_default() -= 1;
                }
                for corner in tri.iter_mut() {
                    if *corner == v {
                        *corner = u;
                    }
                }
                if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
                    *triangle = None;
                    live_triangles -= 1;
                } else {
                    for edge in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                        let key = (edge.0.min(edge.1), edge.0.max(edge.1));
                        *edge_live.entry(key).or_default() += 1;
                    }
                }
            }

            for neighbor in neighbors[u as usize].iter().copied() {
                if neighbor != u && alive[neighbor as usize] {
                    let midpoint = (positions[u as usize] + positions[neighbor as usize]) * 0.5;
                    let extent = positions[u as usize].distance(positions[neighbor as usize]);
                    let combined = quadrics[u as usize].add(quadrics[neighbor as usize]);
                    let candidate = combined.optimal(midpoint, extent);
                    let cost = combined.evaluate(candidate);
                    heap.push(std::cmp::Reverse((
                        cost.to_bits(),
                        u.min(neighbor),
                        u.max(neighbor),
                    )));
                }
            }
        }

        let mut remap = vec![u32::MAX; positions.len()];
        let mut out_positions = Vec::new();
        let mut out_normals = Vec::new();
        for triangle in triangles.iter().flatten() {
            for vertex in triangle {
                let vertex = *vertex as usize;
                if remap[vertex] == u32::MAX {
                    remap[vertex] = out_positions.len() as u32;
                    out_positions.push(positions[vertex].to_array());
                    out_normals.push(normals[vertex].to_array());
                }
            }
        }
        let mut indices = Vec::with_capacity(live_triangles * 3);
        for triangle in triangles.iter().flatten() {
            indices.extend_from_slice(&[
                remap[triangle[0] as usize],
                remap[triangle[1] as usize],
                remap[triangle[2] as usize],
            ]);
        }
        Mesh {
            positions: out_positions,
            normals: out_normals,
            indices,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// Boundary and crease edges above the given dihedral angle.
    pub fn feature_edges(&self, crease_degrees: f32) -> Vec<[u32; 2]> {
        let threshold = crease_degrees.clamp(0.0, 180.0).to_radians().cos();
        let mut edge_faces: HashMap<(u32, u32), (usize, Vec<Vec3>)> = HashMap::new();
        for triangle in self.indices.chunks_exact(3) {
            let a = Vec3::from_array(self.positions[triangle[0] as usize]);
            let b = Vec3::from_array(self.positions[triangle[1] as usize]);
            let c = Vec3::from_array(self.positions[triangle[2] as usize]);
            let normal = (b - a).cross(c - a).normalize_or_zero();
            for edge in [
                (triangle[0], triangle[1]),
                (triangle[1], triangle[2]),
                (triangle[2], triangle[0]),
            ] {
                let key = (edge.0.min(edge.1), edge.0.max(edge.1));
                let entry = edge_faces.entry(key).or_insert((0, Vec::new()));
                entry.0 += 1;
                if normal.length_squared() > 0.5 {
                    entry.1.push(normal);
                }
            }
        }
        let mut edges: Vec<[u32; 2]> = edge_faces
            .into_iter()
            .filter(|(_, (count, normals))| {
                *count == 1
                    || (*count == 2 && normals.len() == 2 && normals[0].dot(normals[1]) < threshold)
            })
            .map(|((u, v), _)| [u, v])
            .collect();
        edges.sort_unstable();
        edges
    }

    /// Connected components of the feature-edge graph.
    pub fn feature_chains(&self, crease_degrees: f32) -> Vec<Vec<u32>> {
        let edges = self.feature_edges(crease_degrees);
        let mut adjacency: HashMap<u32, Vec<u32>> = HashMap::new();
        for [u, v] in &edges {
            adjacency.entry(*u).or_default().push(*v);
            adjacency.entry(*v).or_default().push(*u);
        }
        let mut visited = std::collections::HashSet::new();
        let mut chains = Vec::new();
        for vertex in adjacency.keys().copied() {
            if !visited.insert(vertex) {
                continue;
            }
            let mut chain = Vec::new();
            let mut stack = vec![vertex];
            while let Some(current) = stack.pop() {
                chain.push(current);
                if let Some(neighbors) = adjacency.get(&current) {
                    for neighbor in neighbors {
                        if visited.insert(*neighbor) {
                            stack.push(*neighbor);
                        }
                    }
                }
            }
            chain.sort_unstable();
            chains.push(chain);
        }
        chains.sort_by_key(|chain| chain.first().copied().unwrap_or(u32::MAX));
        chains
    }

    /// Laplacian smoothing restricted to interior crease-chain vertices.
    ///
    /// Only feature vertices with exactly two feature neighbors are moved, and
    /// open-boundary vertices are skipped. Topology is unchanged.
    pub fn relax_feature_chains(&self, crease_degrees: f32, iterations: u32) -> Mesh {
        if iterations == 0 || self.indices.is_empty() {
            return self.clone();
        }
        let mut edge_count: HashMap<(u32, u32), usize> = HashMap::new();
        for triangle in self.indices.chunks_exact(3) {
            for edge in [
                (triangle[0], triangle[1]),
                (triangle[1], triangle[2]),
                (triangle[2], triangle[0]),
            ] {
                let key = (edge.0.min(edge.1), edge.0.max(edge.1));
                *edge_count.entry(key).or_default() += 1;
            }
        }
        let mut boundary = vec![false; self.positions.len()];
        for ((u, v), count) in &edge_count {
            if *count == 1 {
                boundary[*u as usize] = true;
                boundary[*v as usize] = true;
            }
        }
        let mut adjacency: HashMap<u32, Vec<u32>> = HashMap::new();
        for [u, v] in self.feature_edges(crease_degrees) {
            adjacency.entry(u).or_default().push(v);
            adjacency.entry(v).or_default().push(u);
        }
        let mut positions: Vec<Vec3> = self
            .positions
            .iter()
            .map(|position| Vec3::from_array(*position))
            .collect();
        for _ in 0..iterations {
            let previous = positions.clone();
            for (vertex, neighbors) in &adjacency {
                let vertex = *vertex as usize;
                if boundary[vertex] || neighbors.len() != 2 {
                    continue;
                }
                let target =
                    (previous[neighbors[0] as usize] + previous[neighbors[1] as usize]) * 0.5;
                positions[vertex] = previous[vertex].lerp(target, 0.5);
            }
        }
        let mut normals = vec![Vec3::ZERO; positions.len()];
        for triangle in self.indices.chunks_exact(3) {
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
        Mesh {
            positions: positions
                .iter()
                .map(|position| position.to_array())
                .collect(),
            normals: normals.iter().map(|normal| normal.to_array()).collect(),
            indices: self.indices.clone(),
        }
    }

    /// Split vertex normals across creases while keeping the same geometry.
    ///
    /// Triangles are preserved; vertices at sharp edges/corners are duplicated
    /// so each side can carry its own smoothed normal. Use `Mesh::stats()` on
    /// the original mesh, not the split result, for closedness diagnostics.
    pub fn with_crease_normals(&self, crease_degrees: f32) -> Mesh {
        if self.indices.is_empty() {
            return self.clone();
        }
        let threshold = crease_degrees.clamp(0.0, 180.0).to_radians().cos();
        let mut face_normals = Vec::with_capacity(self.indices.len() / 3);
        let mut incident = vec![Vec::new(); self.positions.len()];
        for (face, triangle) in self.indices.chunks_exact(3).enumerate() {
            let a = Vec3::from_array(self.positions[triangle[0] as usize]);
            let b = Vec3::from_array(self.positions[triangle[1] as usize]);
            let c = Vec3::from_array(self.positions[triangle[2] as usize]);
            let normal = (b - a).cross(c - a).normalize_or_zero();
            face_normals.push(if normal.length_squared() > 0.5 {
                normal
            } else {
                Vec3::Y
            });
            for corner in triangle {
                incident[*corner as usize].push(face);
            }
        }

        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut clusters: Vec<Vec<(Vec3, u32)>> = vec![Vec::new(); self.positions.len()];
        for vertex in 0..self.positions.len() {
            let mut local: Vec<(Vec3, Vec<usize>)> = Vec::new();
            for face in &incident[vertex] {
                let normal = face_normals[*face];
                if let Some((_, faces)) = local
                    .iter_mut()
                    .find(|(existing, _)| existing.dot(normal) >= threshold)
                {
                    faces.push(*face);
                } else {
                    local.push((normal, vec![*face]));
                }
            }
            for (_, faces) in local {
                let mut normal = Vec3::ZERO;
                for face in faces {
                    normal += face_normals[face];
                }
                let normal = if normal.length_squared() > 1e-12 {
                    normal.normalize()
                } else {
                    Vec3::Y
                };
                let index = positions.len() as u32;
                positions.push(self.positions[vertex]);
                normals.push(normal.to_array());
                clusters[vertex].push((normal, index));
            }
        }

        let mut indices = Vec::with_capacity(self.indices.len());
        for (face, triangle) in self.indices.chunks_exact(3).enumerate() {
            let normal = face_normals[face];
            for corner in triangle {
                let options = &clusters[*corner as usize];
                let index = options
                    .iter()
                    .max_by(|left, right| left.0.dot(normal).total_cmp(&right.0.dot(normal)))
                    .map(|(_, index)| *index)
                    .unwrap_or(*corner);
                indices.push(index);
            }
        }

        Mesh {
            positions,
            normals,
            indices,
        }
    }

    pub fn bounds(&self) -> Aabb {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for p in &self.positions {
            let p = Vec3::from_array(*p);
            min = min.min(p);
            max = max.max(p);
        }
        if min.x > max.x {
            Aabb::EMPTY
        } else {
            Aabb { min, max }
        }
    }

    /// Topology and orientation diagnostics. `is_closed` requires every edge to
    /// be used exactly twice and every edge to have exactly two incident faces.
    pub fn stats(&self) -> MeshStats {
        let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
        let mut volume = 0.0f64;

        for triangle in self.indices.chunks_exact(3) {
            let a = triangle[0];
            let b = triangle[1];
            let c = triangle[2];

            for (u, v) in [(a, b), (b, c), (c, a)] {
                let key = if u < v { (u, v) } else { (v, u) };
                *edges.entry(key).or_insert(0) += 1;
            }

            let pa = Vec3::from_array(self.positions[a as usize]);
            let pb = Vec3::from_array(self.positions[b as usize]);
            let pc = Vec3::from_array(self.positions[c as usize]);
            volume += (pa.dot(pb.cross(pc)) / 6.0) as f64;
        }

        let mut boundary_edges = 0;
        let mut non_manifold_edges = 0;
        for count in edges.values() {
            if *count == 1 {
                boundary_edges += 1;
            } else if *count > 2 {
                non_manifold_edges += 1;
            }
        }

        MeshStats {
            vertices: self.positions.len(),
            triangles: self.triangle_count(),
            boundary_edges,
            non_manifold_edges,
            is_closed: boundary_edges == 0 && non_manifold_edges == 0 && !self.indices.is_empty(),
            signed_volume: volume,
        }
    }

    /// Cap every simple boundary loop with a centroid fan.
    ///
    /// Non-manifold edges and non-simple boundary vertices are left untouched;
    /// this is a display/export helper, not a watertight remesher.
    pub fn cap_boundary_loops(&self) -> Mesh {
        self.cap_boundary_loops_with_report().0
    }

    pub fn cap_boundary_loops_with_report(&self) -> (Mesh, BoundaryCapReport) {
        let mut edge_faces: HashMap<(u32, u32), Vec<(u32, u32)>> = HashMap::new();
        for triangle in self.indices.chunks_exact(3) {
            let a = triangle[0];
            let b = triangle[1];
            let c = triangle[2];
            for (u, v) in [(a, b), (b, c), (c, a)] {
                let key = if u < v { (u, v) } else { (v, u) };
                edge_faces.entry(key).or_default().push((u, v));
            }
        }
        let boundary_edges = edge_faces
            .values()
            .filter(|directed| directed.len() == 1)
            .count();
        let non_manifold_edges = edge_faces
            .values()
            .filter(|directed| directed.len() > 2)
            .count();
        let mut outgoing: HashMap<u32, Vec<u32>> = HashMap::new();
        for directed in edge_faces.values() {
            if directed.len() == 1 {
                let (a, b) = directed[0];
                outgoing.entry(a).or_default().push(b);
            }
        }
        let branch_vertices = outgoing
            .values()
            .filter(|targets| targets.len() > 1)
            .count();
        let mut report = BoundaryCapReport {
            boundary_edges,
            non_manifold_edges,
            branch_vertices,
            ..BoundaryCapReport::default()
        };
        if outgoing.is_empty() {
            return (self.clone(), report);
        }

        let mut used: HashSet<(u32, u32)> = HashSet::new();
        let mut loops: Vec<Vec<u32>> = Vec::new();
        for (&start, targets) in &outgoing {
            for &target in targets {
                if used.contains(&(start, target)) {
                    continue;
                }
                let mut boundary = vec![start];
                let mut current = target;
                used.insert((start, target));
                while current != start && boundary.len() <= self.positions.len() + 1 {
                    boundary.push(current);
                    let Some(next) = outgoing.get(&current).and_then(|candidates| {
                        candidates
                            .iter()
                            .copied()
                            .find(|next| !used.contains(&(current, *next)))
                    }) else {
                        break;
                    };
                    used.insert((current, next));
                    current = next;
                }
                if current == start && boundary.len() >= 3 {
                    loops.push(boundary);
                }
            }
        }
        if loops.is_empty() {
            return (self.clone(), report);
        }
        report.loops = loops.len();
        report.skipped_edges = boundary_edges
            .saturating_sub(loops.iter().map(|boundary| boundary.len()).sum::<usize>());

        let mut positions = self.positions.clone();
        let mut indices = self.indices.clone();
        for boundary in loops {
            let mut centroid = Vec3::ZERO;
            for &vertex in &boundary {
                centroid += Vec3::from_array(positions[vertex as usize]);
            }
            centroid /= boundary.len() as f32;
            let center = positions.len() as u32;
            positions.push(centroid.to_array());
            let mut loop_normal = Vec3::ZERO;
            for index in 0..boundary.len() {
                let a = Vec3::from_array(positions[boundary[index] as usize]);
                let b =
                    Vec3::from_array(positions[boundary[(index + 1) % boundary.len()] as usize]);
                loop_normal += a.cross(b);
            }
            for index in 0..boundary.len() {
                let a = boundary[index];
                let b = boundary[(index + 1) % boundary.len()];
                let pa = Vec3::from_array(positions[a as usize]);
                let pb = Vec3::from_array(positions[b as usize]);
                let mut triangle = [center, a, b];
                if (pa - centroid).cross(pb - centroid).dot(loop_normal) < 0.0 {
                    triangle.swap(1, 2);
                }
                indices.extend_from_slice(&triangle);
            }
        }

        let mut normals = vec![[0.0; 3]; positions.len()];
        for triangle in indices.chunks_exact(3) {
            let a = Vec3::from_array(positions[triangle[0] as usize]);
            let b = Vec3::from_array(positions[triangle[1] as usize]);
            let c = Vec3::from_array(positions[triangle[2] as usize]);
            let face = (b - a).cross(c - a);
            for vertex in triangle {
                normals[*vertex as usize] =
                    (Vec3::from_array(normals[*vertex as usize]) + face).to_array();
            }
        }
        for normal in &mut normals {
            let normalized = Vec3::from_array(*normal).normalize_or_zero();
            *normal = if normalized.length_squared() > 0.5 {
                normalized.to_array()
            } else {
                Vec3::Y.to_array()
            };
        }
        (
            Mesh {
                positions,
                normals,
                indices,
            },
            report,
        )
    }

    /// Approximate symmetric Hausdorff distance.
    ///
    /// Each direction samples up to 7 fixed barycentric points per triangle
    /// (vertices, edge midpoints, centroid) and queries a triangle BVH on the
    /// other mesh. This is a golden/QA metric, not a watertight proof.
    pub fn hausdorff_distance(&self, other: &Mesh, samples_per_triangle: u32) -> f64 {
        if self.is_empty() || other.is_empty() {
            return if self.is_empty() && other.is_empty() {
                0.0
            } else {
                f64::INFINITY
            };
        }
        let samples = samples_per_triangle.clamp(1, 7) as usize;
        let other_bvh = Bvh::new(other);
        let self_bvh = Bvh::new(self);
        let forward = sample_hausdorff(self, &other_bvh, samples);
        let backward = sample_hausdorff(other, &self_bvh, samples);
        forward.max(backward) as f64
    }

    /// Hausdorff approximation with recursive triangle subdivision.
    ///
    /// Leaves have edge lengths at most `tolerance`; the sampled maximum is
    /// therefore within roughly one tolerance of the continuous one-sided
    /// distance. This is still an approximation, not an interval certificate.
    pub fn hausdorff_distance_tolerance(&self, other: &Mesh, tolerance: f32) -> f64 {
        if self.is_empty() || other.is_empty() {
            return if self.is_empty() && other.is_empty() {
                0.0
            } else {
                f64::INFINITY
            };
        }
        let tolerance = tolerance.max(1e-6);
        let other_bvh = Bvh::new(other);
        let self_bvh = Bvh::new(self);
        let forward = sample_hausdorff_tolerance(self, &other_bvh, tolerance);
        let backward = sample_hausdorff_tolerance(other, &self_bvh, tolerance);
        forward.max(backward) as f64
    }

    /// Conservative interval for the symmetric continuous Hausdorff distance.
    ///
    /// The lower bound is the maximum sampled distance. The upper bound adds
    /// half the leaf triangle's maximum edge length, using the 1-Lipschitz
    /// property of point-to-surface distance and the four midpoint
    /// sub-triangles as a covering argument.
    pub fn hausdorff_bounds(&self, other: &Mesh, tolerance: f32) -> HausdorffBounds {
        if self.is_empty() || other.is_empty() {
            return if self.is_empty() && other.is_empty() {
                HausdorffBounds {
                    lower: 0.0,
                    upper: 0.0,
                }
            } else {
                HausdorffBounds {
                    lower: f64::INFINITY,
                    upper: f64::INFINITY,
                }
            };
        }
        let tolerance = tolerance.max(1e-6);
        let other_bvh = Bvh::new(other);
        let self_bvh = Bvh::new(self);
        let (forward_lower, forward_upper) = sample_hausdorff_bounds(self, &other_bvh, tolerance);
        let (backward_lower, backward_upper) = sample_hausdorff_bounds(other, &self_bvh, tolerance);
        HausdorffBounds {
            lower: forward_lower.max(backward_lower) as f64,
            upper: forward_upper.max(backward_upper) as f64,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HausdorffBounds {
    pub lower: f64,
    pub upper: f64,
}

/// Packed mesh ABI shared by wasm and napi.
///
/// `ZSM2 | u32 vertices | u32 triangles | u32 feature_edges | positions |
/// normals | indices | feature edge indices`.
pub fn encode_packed_mesh(mesh: &Mesh, feature_angle: f32) -> Vec<u8> {
    let vertices = mesh.positions.len() as u32;
    let triangles = mesh.triangle_count() as u32;
    let features = mesh.feature_edges(feature_angle);
    let mut bytes = Vec::with_capacity(
        16 + mesh.positions.len() * 24 + mesh.indices.len() * 4 + features.len() * 8,
    );
    bytes.extend_from_slice(b"ZSM2");
    bytes.extend_from_slice(&vertices.to_le_bytes());
    bytes.extend_from_slice(&triangles.to_le_bytes());
    bytes.extend_from_slice(&(features.len() as u32).to_le_bytes());
    for position in &mesh.positions {
        for component in position {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    for normal in &mesh.normals {
        for component in normal {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    for index in &mesh.indices {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    for edge in &features {
        for vertex in edge {
            bytes.extend_from_slice(&vertex.to_le_bytes());
        }
    }
    bytes
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MeshStats {
    pub vertices: usize,
    pub triangles: usize,
    pub boundary_edges: usize,
    pub non_manifold_edges: usize,
    pub is_closed: bool,
    pub signed_volume: f64,
}

#[derive(Clone)]
struct BvhTriangle {
    a: Vec3,
    b: Vec3,
    c: Vec3,
    bounds: Aabb,
    centroid: Vec3,
}

struct BvhNode {
    bounds: Aabb,
    start: usize,
    count: usize,
    left: i32,
    right: i32,
}

struct Bvh {
    triangles: Vec<BvhTriangle>,
    order: Vec<usize>,
    nodes: Vec<BvhNode>,
}

impl Bvh {
    fn new(mesh: &Mesh) -> Self {
        let triangles: Vec<BvhTriangle> = mesh
            .indices
            .chunks_exact(3)
            .map(|triangle| {
                let a = Vec3::from_array(mesh.positions[triangle[0] as usize]);
                let b = Vec3::from_array(mesh.positions[triangle[1] as usize]);
                let c = Vec3::from_array(mesh.positions[triangle[2] as usize]);
                let bounds = Aabb::new(a.min(b).min(c), a.max(b).max(c));
                BvhTriangle {
                    a,
                    b,
                    c,
                    bounds,
                    centroid: (a + b + c) / 3.0,
                }
            })
            .collect();
        let mut order: Vec<usize> = (0..triangles.len()).collect();
        let mut nodes = Vec::new();
        if !triangles.is_empty() {
            build_bvh(&mut order, 0, &triangles, &mut nodes);
        }
        Self {
            triangles,
            order,
            nodes,
        }
    }

    fn closest_distance(&self, point: Vec3) -> f32 {
        if self.nodes.is_empty() {
            return f32::INFINITY;
        }
        let mut best = f32::INFINITY;
        self.closest_node(point, 0, &mut best);
        best.sqrt()
    }

    fn closest_node(&self, point: Vec3, index: usize, best: &mut f32) {
        let node = &self.nodes[index];
        if aabb_distance_squared(node.bounds, point) >= *best {
            return;
        }
        if node.count > 0 {
            for &triangle_index in &self.order[node.start..node.start + node.count] {
                let triangle = &self.triangles[triangle_index];
                let closest = closest_point_triangle(point, triangle.a, triangle.b, triangle.c);
                let distance = (point - closest).length_squared();
                if distance < *best {
                    *best = distance;
                }
            }
            return;
        }
        let left = self.nodes[node.left as usize].bounds;
        let right = self.nodes[node.right as usize].bounds;
        let left_distance = aabb_distance_squared(left, point);
        let right_distance = aabb_distance_squared(right, point);
        let (near, far) = if left_distance <= right_distance {
            (node.left as usize, node.right as usize)
        } else {
            (node.right as usize, node.left as usize)
        };
        self.closest_node(point, near, best);
        self.closest_node(point, far, best);
    }
}

fn build_bvh(
    order: &mut [usize],
    base: usize,
    triangles: &[BvhTriangle],
    nodes: &mut Vec<BvhNode>,
) -> usize {
    let mut bounds = Aabb::EMPTY;
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    for &index in order.iter() {
        bounds = bounds.union(triangles[index].bounds);
        minimum = minimum.min(triangles[index].centroid);
        maximum = maximum.max(triangles[index].centroid);
    }
    let node_index = nodes.len();
    nodes.push(BvhNode {
        bounds,
        start: base,
        count: order.len(),
        left: -1,
        right: -1,
    });
    if order.len() <= 8 {
        return node_index;
    }

    let extent = maximum - minimum;
    let axis = if extent.x >= extent.y && extent.x >= extent.z {
        0
    } else if extent.y >= extent.z {
        1
    } else {
        2
    };
    order.sort_by(|a, b| {
        triangles[*a].centroid[axis]
            .partial_cmp(&triangles[*b].centroid[axis])
            .unwrap_or(Ordering::Equal)
    });
    let middle = order.len() / 2;
    let left = build_bvh(&mut order[..middle], base, triangles, nodes);
    let right = build_bvh(&mut order[middle..], base + middle, triangles, nodes);
    nodes[node_index].count = 0;
    nodes[node_index].left = left as i32;
    nodes[node_index].right = right as i32;
    node_index
}

fn sample_hausdorff(source: &Mesh, target: &Bvh, samples: usize) -> f32 {
    const PATTERNS: [(f32, f32); 7] = [
        (1.0 / 3.0, 1.0 / 3.0),
        (0.0, 0.0),
        (1.0, 0.0),
        (0.0, 1.0),
        (0.5, 0.0),
        (0.0, 0.5),
        (0.5, 0.5),
    ];
    let mut maximum = 0.0f32;
    for triangle in source.indices.chunks_exact(3) {
        let a = Vec3::from_array(source.positions[triangle[0] as usize]);
        let b = Vec3::from_array(source.positions[triangle[1] as usize]);
        let c = Vec3::from_array(source.positions[triangle[2] as usize]);
        let ab = b - a;
        let ac = c - a;
        for &(u, v) in &PATTERNS[..samples] {
            let point = a + ab * u + ac * v;
            maximum = maximum.max(target.closest_distance(point));
        }
    }
    maximum
}

fn sample_hausdorff_tolerance(source: &Mesh, target: &Bvh, tolerance: f32) -> f32 {
    let mut maximum = 0.0f32;
    for triangle in source.indices.chunks_exact(3) {
        let a = Vec3::from_array(source.positions[triangle[0] as usize]);
        let b = Vec3::from_array(source.positions[triangle[1] as usize]);
        let c = Vec3::from_array(source.positions[triangle[2] as usize]);
        maximum = maximum.max(triangle_distance_tolerance(a, b, c, target, tolerance, 0));
    }
    maximum
}

fn triangle_distance_tolerance(
    a: Vec3,
    b: Vec3,
    c: Vec3,
    target: &Bvh,
    tolerance: f32,
    depth: u32,
) -> f32 {
    let max_edge = (b - a).length().max((c - b).length()).max((a - c).length());
    if max_edge <= tolerance || depth >= 12 {
        let centroid = (a + b + c) / 3.0;
        return target
            .closest_distance(a)
            .max(target.closest_distance(b))
            .max(target.closest_distance(c))
            .max(target.closest_distance(centroid));
    }
    let ab = (a + b) * 0.5;
    let bc = (b + c) * 0.5;
    let ca = (c + a) * 0.5;
    triangle_distance_tolerance(a, ab, ca, target, tolerance, depth + 1)
        .max(triangle_distance_tolerance(
            ab,
            b,
            bc,
            target,
            tolerance,
            depth + 1,
        ))
        .max(triangle_distance_tolerance(
            ca,
            bc,
            c,
            target,
            tolerance,
            depth + 1,
        ))
        .max(triangle_distance_tolerance(
            ab,
            bc,
            ca,
            target,
            tolerance,
            depth + 1,
        ))
}

fn sample_hausdorff_bounds(source: &Mesh, target: &Bvh, tolerance: f32) -> (f32, f32) {
    let mut lower = 0.0f32;
    let mut upper = 0.0f32;
    for triangle in source.indices.chunks_exact(3) {
        let a = Vec3::from_array(source.positions[triangle[0] as usize]);
        let b = Vec3::from_array(source.positions[triangle[1] as usize]);
        let c = Vec3::from_array(source.positions[triangle[2] as usize]);
        let (local_lower, local_upper) = triangle_distance_bounds(a, b, c, target, tolerance, 0);
        lower = lower.max(local_lower);
        upper = upper.max(local_upper);
    }
    (lower, upper)
}

fn triangle_distance_bounds(
    a: Vec3,
    b: Vec3,
    c: Vec3,
    target: &Bvh,
    tolerance: f32,
    depth: u32,
) -> (f32, f32) {
    let max_edge = (b - a).length().max((c - b).length()).max((a - c).length());
    if max_edge <= tolerance || depth >= 12 {
        let centroid = (a + b + c) / 3.0;
        let ab = (a + b) * 0.5;
        let bc = (b + c) * 0.5;
        let ca = (c + a) * 0.5;
        let maximum = target
            .closest_distance(a)
            .max(target.closest_distance(b))
            .max(target.closest_distance(c))
            .max(target.closest_distance(ab))
            .max(target.closest_distance(bc))
            .max(target.closest_distance(ca))
            .max(target.closest_distance(centroid));
        // The four midpoint sub-triangles cover the leaf and each has max edge
        // <= max_edge / 2, so every point is within max_edge / 2 of a sample.
        return (maximum, maximum + max_edge * 0.5);
    }
    let ab = (a + b) * 0.5;
    let bc = (b + c) * 0.5;
    let ca = (c + a) * 0.5;
    let children = [
        triangle_distance_bounds(a, ab, ca, target, tolerance, depth + 1),
        triangle_distance_bounds(ab, b, bc, target, tolerance, depth + 1),
        triangle_distance_bounds(ca, bc, c, target, tolerance, depth + 1),
        triangle_distance_bounds(ab, bc, ca, target, tolerance, depth + 1),
    ];
    let lower = children.iter().map(|child| child.0).fold(0.0, f32::max);
    let upper = children.iter().map(|child| child.1).fold(0.0, f32::max);
    (lower, upper)
}

fn aabb_distance_squared(bounds: Aabb, point: Vec3) -> f32 {
    let closest = point.clamp(bounds.min, bounds.max);
    (point - closest).length_squared()
}

fn closest_point_triangle(point: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let ab = b - a;
    let ac = c - a;
    let ap = point - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = point - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return a + ab * v;
    }
    let cp = point - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return a + ac * w;
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return b + (c - b) * w;
    }
    let denominator = 1.0 / (va + vb + vc);
    let v = vb * denominator;
    let w = vc * denominator;
    a + ab * v + ac * w
}

/// Surface Nets over a dense grid.///
/// One vertex per sign-changing cell (average of edge crossings), one quad per
/// sign-changing grid edge. Quads are split into triangles and the whole mesh
/// is flipped if its signed volume is negative, which gives outward-facing
/// triangles for closed surfaces without per-face gradient queries.
fn surface_nets(grid: &GridSamples) -> Result<Mesh, KernelError> {
    let [nx, ny, nz] = grid.dims;
    if nx < 2 || ny < 2 || nz < 2 {
        return Err(KernelError::GridTooSmall);
    }

    let ncx = nx - 1;
    let ncy = ny - 1;
    let ncz = nz - 1;
    let cell_count = ncx * ncy * ncz;

    let cell_index = |x: usize, y: usize, z: usize| -> usize { x + ncx * (y + ncy * z) };

    let corners: [(usize, usize, usize); 8] = [
        (0, 0, 0),
        (1, 0, 0),
        (1, 1, 0),
        (0, 1, 0),
        (0, 0, 1),
        (1, 0, 1),
        (1, 1, 1),
        (0, 1, 1),
    ];

    let edges: [(usize, usize); 12] = [
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

    let mut cell_vertices = vec![u32::MAX; cell_count];
    let mut positions: Vec<Vec3> = Vec::new();

    for z in 0..ncz {
        for y in 0..ncy {
            for x in 0..ncx {
                let mut values = [0.0f32; 8];
                let mut mask = 0u8;
                for (i, (dx, dy, dz)) in corners.iter().enumerate() {
                    let value = grid.value(x + dx, y + dy, z + dz);
                    values[i] = value;
                    if value < 0.0 {
                        mask |= 1 << i;
                    }
                }

                if mask == 0 || mask == 0xFF {
                    continue;
                }

                let mut sum = Vec3::ZERO;
                let mut crossings = 0u32;
                for (a, b) in edges {
                    let va = values[a];
                    let vb = values[b];
                    if (va < 0.0) == (vb < 0.0) {
                        continue;
                    }
                    let denom = va - vb;
                    let t = if denom.abs() > f32::EPSILON {
                        (va / denom).clamp(0.0, 1.0)
                    } else {
                        0.5
                    };
                    let pa = grid.position(x + corners[a].0, y + corners[a].1, z + corners[a].2);
                    let pb = grid.position(x + corners[b].0, y + corners[b].1, z + corners[b].2);
                    sum += pa.lerp(pb, t);
                    crossings += 1;
                }

                if crossings == 0 {
                    continue;
                }

                let vertex = sum / crossings as f32;
                let index = positions.len() as u32;
                positions.push(vertex);
                cell_vertices[cell_index(x, y, z)] = index;
            }
        }
    }

    let mut indices: Vec<u32> = Vec::new();
    let mut push_quad = |cells: [(usize, usize, usize); 4], flip: bool| {
        let mut quad = [0u32; 4];
        for (i, (x, y, z)) in cells.iter().enumerate() {
            let vertex = cell_vertices[cell_index(*x, *y, *z)];
            if vertex == u32::MAX {
                return;
            }
            quad[i] = vertex;
        }
        if flip {
            quad.reverse();
        }
        indices.extend_from_slice(&[quad[0], quad[1], quad[2], quad[0], quad[2], quad[3]]);
    };

    // Edges along +X: four surrounding cells vary in y and z. This order has a
    // +X normal, so it is kept when the edge starts inside (negative).
    for z in 1..ncz {
        for y in 1..ncy {
            for x in 0..ncx {
                let v0 = grid.value(x, y, z);
                let v1 = grid.value(x + 1, y, z);
                if (v0 < 0.0) != (v1 < 0.0) {
                    push_quad(
                        [(x, y - 1, z - 1), (x, y, z - 1), (x, y, z), (x, y - 1, z)],
                        v0 > 0.0,
                    );
                }
            }
        }
    }

    // Edges along +Y: this order has a +Y normal.
    for z in 1..ncz {
        for x in 1..ncx {
            for y in 0..ncy {
                let v0 = grid.value(x, y, z);
                let v1 = grid.value(x, y + 1, z);
                if (v0 < 0.0) != (v1 < 0.0) {
                    push_quad(
                        [(x - 1, y, z - 1), (x - 1, y, z), (x, y, z), (x, y, z - 1)],
                        v0 > 0.0,
                    );
                }
            }
        }
    }

    // Edges along +Z: this order has a +Z normal.
    for y in 1..ncy {
        for x in 1..ncx {
            for z in 0..ncz {
                let v0 = grid.value(x, y, z);
                let v1 = grid.value(x, y, z + 1);
                if (v0 < 0.0) != (v1 < 0.0) {
                    push_quad(
                        [(x - 1, y - 1, z), (x, y - 1, z), (x, y, z), (x - 1, y, z)],
                        v0 > 0.0,
                    );
                }
            }
        }
    }

    if indices.is_empty() {
        return Ok(Mesh::default());
    }

    // Orient the closed mesh outward: negative inside means the gradient points
    // outward, and a correctly wound closed mesh has positive signed volume.
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

    for normal in &mut normals {
        *normal = normal.normalize_or_zero();
        if normal.length_squared() < 0.5 {
            *normal = Vec3::Y;
        }
    }

    Ok(Mesh {
        positions: positions.iter().map(|p| p.to_array()).collect(),
        normals: normals.iter().map(|n| n.to_array()).collect(),
        indices,
    })
}

fn surface_nets_f64_raw(grid: &GridSamplesF64) -> Result<(Vec<DVec3>, Vec<u32>), KernelError> {
    let [nx, ny, nz] = grid.dims;
    if nx < 2 || ny < 2 || nz < 2 {
        return Err(KernelError::GridTooSmall);
    }
    let ncx = nx - 1;
    let ncy = ny - 1;
    let ncz = nz - 1;
    let cell_index = |x: usize, y: usize, z: usize| -> usize { x + ncx * (y + ncy * z) };
    let corners: [(usize, usize, usize); 8] = [
        (0, 0, 0),
        (1, 0, 0),
        (1, 1, 0),
        (0, 1, 0),
        (0, 0, 1),
        (1, 0, 1),
        (1, 1, 1),
        (0, 1, 1),
    ];
    let edges: [(usize, usize); 12] = [
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

    let mut cell_vertices = vec![u32::MAX; ncx * ncy * ncz];
    let mut positions: Vec<DVec3> = Vec::new();
    for z in 0..ncz {
        for y in 0..ncy {
            for x in 0..ncx {
                let mut values = [0.0f64; 8];
                let mut mask = 0u8;
                for (index, (dx, dy, dz)) in corners.iter().enumerate() {
                    let value = grid.value(x + dx, y + dy, z + dz);
                    values[index] = value;
                    if value < 0.0 {
                        mask |= 1 << index;
                    }
                }
                if mask == 0 || mask == 0xFF {
                    continue;
                }
                let mut sum = DVec3::ZERO;
                let mut crossings = 0u32;
                for (a, b) in edges {
                    let va = values[a];
                    let vb = values[b];
                    if (va < 0.0) == (vb < 0.0) {
                        continue;
                    }
                    let denominator = va - vb;
                    let t = if denominator.abs() > 1e-15 {
                        (va / denominator).clamp(0.0, 1.0)
                    } else {
                        0.5
                    };
                    let pa = grid.position(x + corners[a].0, y + corners[a].1, z + corners[a].2);
                    let pb = grid.position(x + corners[b].0, y + corners[b].1, z + corners[b].2);
                    sum += pa.lerp(pb, t);
                    crossings += 1;
                }
                if crossings == 0 {
                    continue;
                }
                cell_vertices[cell_index(x, y, z)] = positions.len() as u32;
                positions.push(sum / crossings as f64);
            }
        }
    }

    let mut indices: Vec<u32> = Vec::new();
    let mut push_quad = |cells: [(usize, usize, usize); 4], flip: bool| {
        let mut quad = [0u32; 4];
        for (index, (x, y, z)) in cells.iter().enumerate() {
            let vertex = cell_vertices[cell_index(*x, *y, *z)];
            if vertex == u32::MAX {
                return;
            }
            quad[index] = vertex;
        }
        if flip {
            quad.reverse();
        }
        indices.extend_from_slice(&[quad[0], quad[1], quad[2], quad[0], quad[2], quad[3]]);
    };
    for z in 1..ncz {
        for y in 1..ncy {
            for x in 0..ncx {
                let v0 = grid.value(x, y, z);
                let v1 = grid.value(x + 1, y, z);
                if (v0 < 0.0) != (v1 < 0.0) {
                    push_quad(
                        [(x, y - 1, z - 1), (x, y, z - 1), (x, y, z), (x, y - 1, z)],
                        v0 > 0.0,
                    );
                }
            }
        }
    }
    for z in 1..ncz {
        for x in 1..ncx {
            for y in 0..ncy {
                let v0 = grid.value(x, y, z);
                let v1 = grid.value(x, y + 1, z);
                if (v0 < 0.0) != (v1 < 0.0) {
                    push_quad(
                        [(x - 1, y, z - 1), (x - 1, y, z), (x, y, z), (x, y, z - 1)],
                        v0 > 0.0,
                    );
                }
            }
        }
    }
    for y in 1..ncy {
        for x in 1..ncx {
            for z in 0..ncz {
                let v0 = grid.value(x, y, z);
                let v1 = grid.value(x, y, z + 1);
                if (v0 < 0.0) != (v1 < 0.0) {
                    push_quad(
                        [(x - 1, y - 1, z), (x, y - 1, z), (x, y, z), (x - 1, y, z)],
                        v0 > 0.0,
                    );
                }
            }
        }
    }
    if indices.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    Ok((positions, indices))
}

fn finalize_mesh_f64(positions: Vec<DVec3>, mut indices: Vec<u32>, reference: DVec3) -> Mesh {
    let local: Vec<DVec3> = positions.iter().map(|p| *p - reference).collect();
    let mut volume = 0.0f64;
    for triangle in indices.chunks_exact(3) {
        let a = local[triangle[0] as usize];
        let b = local[triangle[1] as usize];
        let c = local[triangle[2] as usize];
        volume += a.dot(b.cross(c)) / 6.0;
    }
    if volume < 0.0 {
        for triangle in indices.chunks_exact_mut(3) {
            triangle.swap(1, 2);
        }
    }
    let positions_f32: Vec<Vec3> = local
        .iter()
        .map(|p| Vec3::new(p.x as f32, p.y as f32, p.z as f32))
        .collect();
    let mut normals = vec![Vec3::ZERO; positions_f32.len()];
    for triangle in indices.chunks_exact(3) {
        let a = positions_f32[triangle[0] as usize];
        let b = positions_f32[triangle[1] as usize];
        let c = positions_f32[triangle[2] as usize];
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
    Mesh {
        positions: positions_f32.iter().map(|p| p.to_array()).collect(),
        normals: normals.iter().map(|n| n.to_array()).collect(),
        indices,
    }
}

fn finalize_mesh_f64_world(positions: Vec<DVec3>, mut indices: Vec<u32>) -> MeshF64 {
    let mut centroid = DVec3::ZERO;
    for position in &positions {
        centroid += *position;
    }
    centroid /= positions.len().max(1) as f64;
    let mut volume = 0.0f64;
    for triangle in indices.chunks_exact(3) {
        let a = positions[triangle[0] as usize] - centroid;
        let b = positions[triangle[1] as usize] - centroid;
        let c = positions[triangle[2] as usize] - centroid;
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
    MeshF64 {
        positions: positions.iter().map(|p| p.to_array()).collect(),
        normals: normals.iter().map(|n| n.to_array()).collect(),
        indices,
    }
}

fn surface_nets_f64(grid: &GridSamplesF64, reference: DVec3) -> Result<Mesh, KernelError> {
    let (positions, indices) = surface_nets_f64_raw(grid)?;
    Ok(finalize_mesh_f64(positions, indices, reference))
}

fn surface_nets_f64_world(grid: &GridSamplesF64) -> Result<MeshF64, KernelError> {
    let (positions, indices) = surface_nets_f64_raw(grid)?;
    Ok(finalize_mesh_f64_world(positions, indices))
}

fn dual_contour_f64_raw<F>(
    grid: &GridSamplesF64,
    gradient: &F,
) -> Result<(Vec<DVec3>, Vec<u32>), KernelError>
where
    F: Fn(DVec3) -> DVec3,
{
    let [nx, ny, nz] = grid.dims;
    if nx < 2 || ny < 2 || nz < 2 {
        return Err(KernelError::GridTooSmall);
    }
    let ncx = nx - 1;
    let ncy = ny - 1;
    let ncz = nz - 1;
    let cell_index = |x: usize, y: usize, z: usize| -> usize { x + ncx * (y + ncy * z) };
    let corners: [(usize, usize, usize); 8] = [
        (0, 0, 0),
        (1, 0, 0),
        (1, 1, 0),
        (0, 1, 0),
        (0, 0, 1),
        (1, 0, 1),
        (1, 1, 1),
        (0, 1, 1),
    ];
    let edges: [(usize, usize); 12] = [
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

    let mut cell_vertices = vec![u32::MAX; ncx * ncy * ncz];
    let mut positions: Vec<DVec3> = Vec::new();
    for z in 0..ncz {
        for y in 0..ncy {
            for x in 0..ncx {
                let mut values = [0.0f64; 8];
                let mut mask = 0u8;
                for (index, (dx, dy, dz)) in corners.iter().enumerate() {
                    let value = grid.value(x + dx, y + dy, z + dz);
                    values[index] = value;
                    if value < 0.0 {
                        mask |= 1 << index;
                    }
                }
                if mask == 0 || mask == 0xFF {
                    continue;
                }

                let mut ata = DMat3::ZERO;
                let mut atb = DVec3::ZERO;
                let mut centroid = DVec3::ZERO;
                let mut crossings = 0u32;
                let mut planes: Vec<(DVec3, f64)> = Vec::new();
                for (a, b) in edges {
                    let va = values[a];
                    let vb = values[b];
                    if (va < 0.0) == (vb < 0.0) {
                        continue;
                    }
                    let denominator = va - vb;
                    let t = if denominator.abs() > 1e-15 {
                        (va / denominator).clamp(0.0, 1.0)
                    } else {
                        0.5
                    };
                    let pa = grid.position(x + corners[a].0, y + corners[a].1, z + corners[a].2);
                    let pb = grid.position(x + corners[b].0, y + corners[b].1, z + corners[b].2);
                    let intersection = pa.lerp(pb, t);
                    centroid += intersection;
                    crossings += 1;

                    let normal = gradient(intersection);
                    if !normal.is_finite() || normal.length_squared() < 1e-24 {
                        continue;
                    }
                    let normal = normal.normalize();
                    let offset = normal.dot(intersection);
                    planes.push((normal, offset));
                    ata +=
                        DMat3::from_cols(normal * normal.x, normal * normal.y, normal * normal.z);
                    atb += normal * offset;
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
                                let weight =
                                    2.0 + 4.0 * (1.0 - n1.dot(n2).max(n1.dot(n3)).max(n2.dot(n3)));
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
                let mut vertex = if determinant.abs() > 1e-18 {
                    ata.inverse() * atb
                } else {
                    centroid
                };
                if !vertex.is_finite() {
                    vertex = centroid;
                }
                let cell_min = grid.position(x, y, z);
                let cell_max = cell_min + DVec3::splat(grid.cell);
                vertex = vertex.clamp(cell_min, cell_max);
                cell_vertices[cell_index(x, y, z)] = positions.len() as u32;
                positions.push(vertex);
            }
        }
    }

    let mut indices: Vec<u32> = Vec::new();
    let mut push_quad = |cells: [(usize, usize, usize); 4], flip: bool| {
        let mut quad = [0u32; 4];
        for (index, (x, y, z)) in cells.iter().enumerate() {
            let vertex = cell_vertices[cell_index(*x, *y, *z)];
            if vertex == u32::MAX {
                return;
            }
            quad[index] = vertex;
        }
        if flip {
            quad.reverse();
        }
        indices.extend_from_slice(&[quad[0], quad[1], quad[2], quad[0], quad[2], quad[3]]);
    };
    for z in 1..ncz {
        for y in 1..ncy {
            for x in 0..ncx {
                let v0 = grid.value(x, y, z);
                let v1 = grid.value(x + 1, y, z);
                if (v0 < 0.0) != (v1 < 0.0) {
                    push_quad(
                        [(x, y - 1, z - 1), (x, y, z - 1), (x, y, z), (x, y - 1, z)],
                        v0 > 0.0,
                    );
                }
            }
        }
    }
    for z in 1..ncz {
        for x in 1..ncx {
            for y in 0..ncy {
                let v0 = grid.value(x, y, z);
                let v1 = grid.value(x, y + 1, z);
                if (v0 < 0.0) != (v1 < 0.0) {
                    push_quad(
                        [(x - 1, y, z - 1), (x - 1, y, z), (x, y, z), (x, y, z - 1)],
                        v0 > 0.0,
                    );
                }
            }
        }
    }
    for y in 1..ncy {
        for x in 1..ncx {
            for z in 0..ncz {
                let v0 = grid.value(x, y, z);
                let v1 = grid.value(x, y, z + 1);
                if (v0 < 0.0) != (v1 < 0.0) {
                    push_quad(
                        [(x - 1, y - 1, z), (x, y - 1, z), (x, y, z), (x - 1, y, z)],
                        v0 > 0.0,
                    );
                }
            }
        }
    }
    if indices.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    Ok((positions, indices))
}

fn dual_contour_f64<F>(
    grid: &GridSamplesF64,
    reference: DVec3,
    gradient: &F,
) -> Result<Mesh, KernelError>
where
    F: Fn(DVec3) -> DVec3,
{
    let (positions, indices) = dual_contour_f64_raw(grid, gradient)?;
    Ok(finalize_mesh_f64(positions, indices, reference))
}

fn dual_contour_f64_world<F>(grid: &GridSamplesF64, gradient: &F) -> Result<MeshF64, KernelError>
where
    F: Fn(DVec3) -> DVec3,
{
    let (positions, indices) = dual_contour_f64_raw(grid, gradient)?;
    Ok(finalize_mesh_f64_world(positions, indices))
}

fn dual_contour<F>(grid: &GridSamples, gradient: &F) -> Result<Mesh, KernelError>
where
    F: Fn(Vec3) -> Vec3,
{
    let [nx, ny, nz] = grid.dims;
    if nx < 2 || ny < 2 || nz < 2 {
        return Err(KernelError::GridTooSmall);
    }

    let ncx = nx - 1;
    let ncy = ny - 1;
    let ncz = nz - 1;
    let cell_count = ncx * ncy * ncz;
    let cell_index = |x: usize, y: usize, z: usize| -> usize { x + ncx * (y + ncy * z) };

    let corners: [(usize, usize, usize); 8] = [
        (0, 0, 0),
        (1, 0, 0),
        (1, 1, 0),
        (0, 1, 0),
        (0, 0, 1),
        (1, 0, 1),
        (1, 1, 1),
        (0, 1, 1),
    ];
    let edges: [(usize, usize); 12] = [
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

    let mut cell_vertices = vec![u32::MAX; cell_count];
    let mut positions: Vec<Vec3> = Vec::new();

    for z in 0..ncz {
        for y in 0..ncy {
            for x in 0..ncx {
                let mut values = [0.0f32; 8];
                let mut mask = 0u8;
                for (index, (dx, dy, dz)) in corners.iter().enumerate() {
                    let value = grid.value(x + dx, y + dy, z + dz);
                    values[index] = value;
                    if value < 0.0 {
                        mask |= 1 << index;
                    }
                }
                if mask == 0 || mask == 0xFF {
                    continue;
                }

                let mut ata = Mat3::ZERO;
                let mut atb = Vec3::ZERO;
                let mut centroid = Vec3::ZERO;
                let mut crossings = 0u32;
                let mut planes: Vec<(Vec3, f32)> = Vec::new();

                for (a, b) in edges {
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
                    let pa = grid.position(x + corners[a].0, y + corners[a].1, z + corners[a].2);
                    let pb = grid.position(x + corners[b].0, y + corners[b].1, z + corners[b].2);
                    let intersection = pa.lerp(pb, t);
                    centroid += intersection;
                    crossings += 1;

                    let normal = gradient(intersection);
                    if !normal.is_finite() || normal.length_squared() < 1e-12 {
                        continue;
                    }
                    let normal = normal.normalize();
                    let offset = normal.dot(intersection);
                    planes.push((normal, offset));
                    ata += Mat3::from_cols(normal * normal.x, normal * normal.y, normal * normal.z);
                    atb += normal * offset;
                }

                if crossings == 0 {
                    continue;
                }

                // Strongly differing edge normals indicate a sharp feature.
                // Add a point-to-line constraint for each distinct normal pair
                // so QEF does not average the two face planes into the edge.
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

                // Three strongly independent normals indicate a sharp corner.
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
                                let weight =
                                    2.0 + 4.0 * (1.0 - n1.dot(n2).max(n1.dot(n3)).max(n2.dot(n3)));
                                ata += Mat3::IDENTITY * weight;
                                atb += corner * weight;
                            }
                        }
                    }
                }

                centroid /= crossings as f32;
                // A small Tikhonov term keeps the 3x3 system stable when edge
                // normals are nearly parallel (planes, flat regions).
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
                let cell_min = grid.position(x, y, z);
                let cell_max = cell_min + Vec3::splat(grid.cell);
                vertex = vertex.clamp(cell_min, cell_max);

                let index = positions.len() as u32;
                positions.push(vertex);
                cell_vertices[cell_index(x, y, z)] = index;
            }
        }
    }

    let mut indices: Vec<u32> = Vec::new();
    let mut push_quad = |cells: [(usize, usize, usize); 4], flip: bool| {
        let mut quad = [0u32; 4];
        for (index, (x, y, z)) in cells.iter().enumerate() {
            let vertex = cell_vertices[cell_index(*x, *y, *z)];
            if vertex == u32::MAX {
                return;
            }
            quad[index] = vertex;
        }
        if flip {
            quad.reverse();
        }
        indices.extend_from_slice(&[quad[0], quad[1], quad[2], quad[0], quad[2], quad[3]]);
    };

    for z in 1..ncz {
        for y in 1..ncy {
            for x in 0..ncx {
                let v0 = grid.value(x, y, z);
                let v1 = grid.value(x + 1, y, z);
                if (v0 < 0.0) != (v1 < 0.0) {
                    push_quad(
                        [(x, y - 1, z - 1), (x, y, z - 1), (x, y, z), (x, y - 1, z)],
                        v0 > 0.0,
                    );
                }
            }
        }
    }
    for z in 1..ncz {
        for x in 1..ncx {
            for y in 0..ncy {
                let v0 = grid.value(x, y, z);
                let v1 = grid.value(x, y + 1, z);
                if (v0 < 0.0) != (v1 < 0.0) {
                    push_quad(
                        [(x - 1, y, z - 1), (x - 1, y, z), (x, y, z), (x, y, z - 1)],
                        v0 > 0.0,
                    );
                }
            }
        }
    }
    for y in 1..ncy {
        for x in 1..ncx {
            for z in 0..ncz {
                let v0 = grid.value(x, y, z);
                let v1 = grid.value(x, y, z + 1);
                if (v0 < 0.0) != (v1 < 0.0) {
                    push_quad(
                        [(x - 1, y - 1, z), (x, y - 1, z), (x, y, z), (x - 1, y, z)],
                        v0 > 0.0,
                    );
                }
            }
        }
    }

    if indices.is_empty() {
        return Ok(Mesh::default());
    }

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
    for normal in &mut normals {
        *normal = normal.normalize_or_zero();
        if normal.length_squared() < 0.5 {
            *normal = Vec3::Y;
        }
    }

    Ok(Mesh {
        positions: positions.iter().map(|p| p.to_array()).collect(),
        normals: normals.iter().map(|n| n.to_array()).collect(),
        indices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sphere_graph(radius: f32) -> FieldGraph {
        let mut graph = FieldGraph::new();
        let sphere = graph.sphere(Vec3::ZERO, radius);
        graph.set_root(sphere);
        graph
    }

    #[test]
    fn grid_layout_and_padding() {
        let graph = sphere_graph(1.0);
        let grid = graph.sample_grid(16).unwrap();
        assert_eq!(grid.values.len(), grid.corner_count());
        assert!(grid.bounds.min.x < -1.0);
        assert!(grid.bounds.max.x > 1.0);
        assert!(grid.dims[0] >= 17);
    }

    #[test]
    fn hausdorff_identical_is_zero_and_translation_is_detected() {
        let graph = crate::presets::sphere();
        let mesh = graph.sample_grid(32).unwrap().surface_nets().unwrap();
        let same = mesh.hausdorff_distance(&mesh, 3);
        assert!(same < 1e-5, "identical Hausdorff {same}");

        let mut moved = mesh.clone();
        for position in &mut moved.positions {
            position[0] += 1.0;
        }
        let distance = mesh.hausdorff_distance(&moved, 3);
        assert!(
            (distance - 1.0).abs() < 0.1,
            "translation Hausdorff {distance}"
        );
    }

    #[test]
    fn hausdorff_tolerance_refines_triangle_sampling() {
        let graph = crate::presets::sphere();
        let mesh = graph.sample_grid(16).unwrap().surface_nets().unwrap();
        let same = mesh.hausdorff_distance_tolerance(&mesh, 0.1);
        assert!(same < 1e-4, "identical tolerance Hausdorff {same}");

        let mut moved = mesh.clone();
        for position in &mut moved.positions {
            position[0] += 1.0;
        }
        let distance = mesh.hausdorff_distance_tolerance(&moved, 0.1);
        assert!(
            (distance - 1.0).abs() < 0.15,
            "translation tolerance Hausdorff {distance}"
        );
    }

    #[test]
    fn hausdorff_bounds_contain_translation_distance() {
        let graph = crate::presets::sphere();
        let mesh = graph.sample_grid(16).unwrap().surface_nets().unwrap();
        let same = mesh.hausdorff_bounds(&mesh, 0.1);
        assert!(same.lower < 1e-4, "identical lower {}", same.lower);
        assert!(same.upper < 0.6, "identical upper {}", same.upper);

        let mut moved = mesh.clone();
        for position in &mut moved.positions {
            position[0] += 1.0;
        }
        let bounds = mesh.hausdorff_bounds(&moved, 0.1);
        assert!(bounds.lower <= 1.0 + 1e-3, "lower {}", bounds.lower);
        assert!(bounds.upper >= 1.0 - 1e-3, "upper {}", bounds.upper);
        assert!(
            bounds.upper - bounds.lower < 0.75,
            "interval too wide: {bounds:?}"
        );
    }

    #[test]
    fn hausdorff_between_surface_nets_and_dual_contour_is_small() {
        let graph = crate::presets::sphere();
        let grid = graph.sample_grid(32).unwrap();
        let surface = grid.surface_nets().unwrap();
        let dual = grid
            .dual_contour_with_gradient(|point| graph.gradient(point))
            .unwrap();
        let distance = surface.hausdorff_distance(&dual, 3);
        assert!(distance < 0.1, "SN/DC Hausdorff {distance}");
    }

    #[test]
    fn f64_surface_nets_keeps_far_model_precise() {
        let center = Vec3::new(1_000_000.0, 200_000.0, -300_000.0);
        let mut graph = FieldGraph::new();
        let node = graph.sphere(center, 1.0);
        graph.set_root(node);

        let f32_grid = graph.sample_grid(64).unwrap();
        let f32_mesh = f32_grid.surface_nets().unwrap();
        let f32_error = f32_mesh
            .positions
            .iter()
            .map(|p| (Vec3::from_array(*p) - center).length() - 1.0)
            .map(f32::abs)
            .fold(0.0f32, f32::max);

        let f64_grid = graph.sample_grid_f64(64).unwrap();
        let reference = DVec3::new(center.x as f64, center.y as f64, center.z as f64);
        let f64_mesh = f64_grid.surface_nets_local(reference).unwrap();
        assert!(f64_mesh.stats().is_closed);
        let f64_error = f64_mesh
            .positions
            .iter()
            .map(|p| Vec3::from_array(*p).length() - 1.0)
            .map(f32::abs)
            .fold(0.0f32, f32::max);

        assert!(
            f64_error < 1e-3,
            "f64 local radius error {f64_error} too high"
        );
        assert!(
            f32_error > 1e-3,
            "expected f32 world quantization error, got {f32_error}"
        );
        assert!(f64_error < f32_error);
    }

    #[test]
    fn f64_dual_contour_is_closed_and_sharp() {
        let center = Vec3::new(1_000_000.0, 200_000.0, -300_000.0);
        let mut graph = FieldGraph::new();
        let node = graph.sphere(center, 1.0);
        graph.set_root(node);
        let reference = DVec3::new(center.x as f64, center.y as f64, center.z as f64);
        let grid = graph.sample_grid_f64(64).unwrap();
        let mesh = grid.dual_contour_local(reference).unwrap();
        assert!(mesh.stats().is_closed);
        let max_error = mesh
            .positions
            .iter()
            .map(|p| (Vec3::from_array(*p).length() - 1.0).abs())
            .fold(0.0f32, f32::max);
        assert!(max_error < 1e-3, "f64 DC radius error {max_error}");

        let mut graph = FieldGraph::new();
        let node = graph.rounded_box(Vec3::ZERO, Vec3::splat(1.0), 0.0);
        graph.set_root(node);
        let grid = graph.sample_grid_f64(64).unwrap();
        let surface = grid.surface_nets_local(DVec3::ZERO).unwrap();
        let dual = grid
            .dual_contour_local_with_gradient(DVec3::ZERO, |point| graph.gradient_f64(point))
            .unwrap();
        let mean = |mesh: &Mesh| {
            let total: f64 = mesh
                .positions
                .iter()
                .map(|p| {
                    graph
                        .evaluate_f64(DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64))
                        .abs()
                })
                .sum();
            total / mesh.positions.len() as f64
        };
        let surface_error = mean(&surface);
        let dual_error = mean(&dual);
        assert!(
            dual_error < surface_error,
            "f64 DC {dual_error} should beat SN {surface_error}"
        );
    }

    #[test]
    fn f64_world_mesh_keeps_world_positions() {
        let center = Vec3::new(1_000_000.0, 200_000.0, -300_000.0);
        let mut graph = FieldGraph::new();
        let node = graph.sphere(center, 1.0);
        graph.set_root(node);
        let center64 = DVec3::new(center.x as f64, center.y as f64, center.z as f64);

        let grid = graph.sample_grid_f64(64).unwrap();
        let mesh = grid.surface_nets_world().unwrap();
        assert!(mesh.triangle_count() > 0);
        let max_error = mesh
            .positions
            .iter()
            .map(|p| (DVec3::from_array(*p).distance(center64) - 1.0).abs())
            .fold(0.0f64, f64::max);
        assert!(max_error < 1e-3, "f64 world radius error {max_error}");

        let local = mesh.to_f32_local(center64);
        assert!(local.stats().is_closed);
        let local_error = local
            .positions
            .iter()
            .map(|p| (Vec3::from_array(*p).length() - 1.0).abs())
            .fold(0.0f32, f32::max);
        assert!(local_error < 1e-3);
    }

    #[test]
    fn f64_world_dual_contour_keeps_world_positions() {
        let center = Vec3::new(1_000_000.0, 200_000.0, -300_000.0);
        let mut graph = FieldGraph::new();
        let node = graph.sphere(center, 1.0);
        graph.set_root(node);
        let center64 = DVec3::new(center.x as f64, center.y as f64, center.z as f64);

        let grid = graph.sample_grid_f64(64).unwrap();
        let mesh = grid
            .dual_contour_world_with_gradient(|point| graph.gradient_f64(point))
            .unwrap();
        assert!(mesh.triangle_count() > 0);
        let max_error = mesh
            .positions
            .iter()
            .map(|p| (DVec3::from_array(*p).distance(center64) - 1.0).abs())
            .fold(0.0f64, f64::max);
        assert!(max_error < 1e-3, "f64 world DC radius error {max_error}");
        assert!(mesh.to_f32_local(center64).stats().is_closed);
    }

    #[test]
    fn dual_contour_sphere_is_closed_and_outward() {
        let graph = crate::presets::sphere();
        let grid = graph.sample_grid(64).unwrap();
        let mesh = grid
            .dual_contour_with_gradient(|point| graph.gradient(point))
            .unwrap();
        let stats = mesh.stats();
        assert!(stats.triangles > 0);
        assert_eq!(stats.boundary_edges, 0);
        assert!(stats.is_closed);
        let expected = 4.0 / 3.0 * std::f32::consts::PI;
        assert!((stats.signed_volume as f32 - expected).abs() < 0.3);
        for position in &mesh.positions {
            let radius = Vec3::from_array(*position).length();
            assert!((radius - 1.0).abs() < 0.15, "radius {radius}");
        }
    }

    #[test]
    fn dual_contour_reduces_sharp_box_error() {
        let mut graph = FieldGraph::new();
        let node = graph.rounded_box(Vec3::ZERO, Vec3::splat(1.0), 0.0);
        graph.set_root(node);
        let grid = graph.sample_grid(64).unwrap();

        let surface = grid.surface_nets().unwrap();
        let dual = grid
            .dual_contour_with_gradient(|point| graph.gradient(point))
            .unwrap();
        assert!(dual.stats().is_closed);
        assert_eq!(dual.stats().boundary_edges, 0);

        let mean = |mesh: &Mesh| {
            let total: f32 = mesh
                .positions
                .iter()
                .map(|position| graph.evaluate(Vec3::from_array(*position)).abs())
                .sum();
            total / mesh.positions.len() as f32
        };
        let surface_error = mean(&surface);
        let dual_error = mean(&dual);
        assert!(
            dual_error < surface_error,
            "dual {dual_error} should beat surface nets {surface_error}"
        );
    }

    #[test]
    fn dual_contour_feature_constraints_reduce_sharp_corner_error() {
        let mut graph = FieldGraph::new();
        let node = graph.rounded_box(Vec3::ZERO, Vec3::splat(1.0), 0.0);
        graph.set_root(node);
        let grid = graph.sample_grid(64).unwrap();
        let dual = grid
            .dual_contour_with_gradient(|point| graph.gradient(point))
            .unwrap();
        let maximum = dual
            .positions
            .iter()
            .map(|position| graph.evaluate(Vec3::from_array(*position)).abs())
            .fold(0.0f32, f32::max);
        assert!(
            maximum < 1e-5,
            "sharp-corner max error should be sub-1e-5, got {maximum}"
        );
    }

    #[test]
    fn crease_normals_split_sharp_box_vertices() {
        let mut graph = FieldGraph::new();
        let node = graph.rounded_box(Vec3::ZERO, Vec3::splat(1.0), 0.0);
        graph.set_root(node);
        let mesh = graph.sample_grid(16).unwrap().surface_nets().unwrap();
        let split = mesh.with_crease_normals(30.0);
        assert_eq!(split.triangle_count(), mesh.triangle_count());
        assert!(split.vertex_count() > mesh.vertex_count());
        assert!(split.normals.iter().all(|normal| {
            let normal = Vec3::from_array(*normal);
            normal.length() > 0.99
        }));
        let axis_aligned = split
            .normals
            .iter()
            .filter(|normal| Vec3::from_array(**normal).abs().max_element() > 0.99)
            .count();
        assert!(
            axis_aligned > split.normals.len() / 2,
            "expected mostly axis-aligned box normals, got {axis_aligned}/{}",
            split.normals.len()
        );
    }

    #[test]
    fn edge_collapse_simplify_reduces_closed_triangle_count() {
        let graph = crate::presets::sphere();
        let mesh = graph.sample_grid(24).unwrap().surface_nets().unwrap();
        let simplified = mesh.simplify_edge_collapse(0.5);
        assert!(simplified.triangle_count() > 0);
        assert!(simplified.triangle_count() < mesh.triangle_count());
        assert!(simplified.stats().is_closed);
        assert_eq!(simplified.stats().boundary_edges, 0);
        assert!(
            simplified
                .positions
                .iter()
                .all(|position| position.iter().all(|value| value.is_finite()))
        );
    }

    #[test]
    fn qem_preserves_sharp_box_shape() {
        let mut graph = FieldGraph::new();
        let node = graph.rounded_box(Vec3::ZERO, Vec3::splat(1.0), 0.0);
        graph.set_root(node);
        let mesh = graph.sample_grid(32).unwrap().surface_nets().unwrap();
        let simplified = mesh.simplify_edge_collapse(0.5);
        let stats = simplified.stats();
        assert!(stats.is_closed);
        assert_eq!(stats.boundary_edges, 0);
        let hausdorff = mesh.hausdorff_distance_tolerance(&simplified, 0.05);
        assert!(hausdorff < 0.2, "sharp box QEM Hausdorff {hausdorff}");
    }

    #[test]
    fn edge_collapse_keeps_open_boundary_finite() {
        let graph = crate::presets::sphere();
        let mesh = graph.sample_grid(16).unwrap().surface_nets().unwrap();
        let mut open = mesh.clone();
        open.indices.drain(0..3);
        let simplified = open.simplify_edge_collapse(0.8);
        assert!(simplified.triangle_count() < open.triangle_count());
        assert!(simplified.stats().boundary_edges > 0);
        assert!(
            simplified
                .positions
                .iter()
                .all(|position| position.iter().all(|value| value.is_finite()))
        );
    }

    #[test]
    fn cap_boundary_loops_closes_open_quad() {
        let mesh = Mesh {
            positions: vec![
                [-1.0, -1.0, 0.0],
                [1.0, -1.0, 0.0],
                [1.0, 1.0, 0.0],
                [-1.0, 1.0, 0.0],
            ],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
        };
        let capped = mesh.cap_boundary_loops();
        let stats = capped.stats();
        assert_eq!(stats.vertices, 5);
        assert_eq!(stats.triangles, 6);
        assert!(stats.is_closed);
        assert_eq!(stats.boundary_edges, 0);
    }

    #[test]
    fn cap_boundary_loops_handles_multiple_loops() {
        let mesh = Mesh {
            positions: vec![
                [-1.0, -1.0, 0.0],
                [1.0, -1.0, 0.0],
                [1.0, 1.0, 0.0],
                [-1.0, 1.0, 0.0],
                [3.0, -1.0, 0.0],
                [5.0, -1.0, 0.0],
                [5.0, 1.0, 0.0],
                [3.0, 1.0, 0.0],
            ],
            normals: vec![[0.0, 0.0, 1.0]; 8],
            indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
        };
        let (capped, report) = mesh.cap_boundary_loops_with_report();
        assert_eq!(report.boundary_edges, 8);
        assert_eq!(report.loops, 2);
        assert_eq!(report.skipped_edges, 0);
        let stats = capped.stats();
        assert_eq!(stats.vertices, 10);
        assert_eq!(stats.triangles, 12);
        assert!(stats.is_closed);
        assert_eq!(stats.boundary_edges, 0);
    }

    #[test]
    fn cap_boundary_report_flags_non_manifold_fan() {
        let mesh = Mesh {
            positions: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, -1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            normals: vec![[0.0, 0.0, 1.0]; 5],
            indices: vec![0, 1, 2, 0, 1, 3, 0, 1, 4],
        };
        let (capped, report) = mesh.cap_boundary_loops_with_report();
        assert_eq!(report.non_manifold_edges, 1);
        assert!(report.branch_vertices >= 1);
        assert!(!capped.stats().is_closed);
    }

    #[test]
    fn packed_mesh_has_zsm2_header_and_features() {
        let graph = crate::presets::sphere();
        let mesh = graph.sample_grid(16).unwrap().surface_nets().unwrap();
        let bytes = encode_packed_mesh(&mesh, 30.0);
        assert_eq!(&bytes[..4], b"ZSM2");
        let vertices = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let triangles = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let features = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        assert_eq!(
            bytes.len(),
            16 + vertices * 24 + triangles * 12 + features * 8
        );
    }

    #[test]
    fn feature_graph_finds_box_creases() {
        let mut graph = FieldGraph::new();
        let node = graph.rounded_box(Vec3::ZERO, Vec3::splat(1.0), 0.0);
        graph.set_root(node);
        let box_mesh = graph.sample_grid(16).unwrap().surface_nets().unwrap();
        let edges = box_mesh.feature_edges(30.0);
        let chains = box_mesh.feature_chains(30.0);
        assert!(!edges.is_empty());
        assert!(!chains.is_empty());
        assert_eq!(
            chains.iter().map(|chain| chain.len()).sum::<usize>(),
            edges
                .iter()
                .flat_map(|edge| edge.iter().copied())
                .collect::<std::collections::HashSet<_>>()
                .len()
        );
    }

    #[test]
    fn feature_chain_relax_keeps_box_closed() {
        let mut graph = FieldGraph::new();
        let node = graph.rounded_box(Vec3::ZERO, Vec3::splat(1.0), 0.0);
        graph.set_root(node);
        let mesh = graph.sample_grid(16).unwrap().surface_nets().unwrap();
        let relaxed = mesh.relax_feature_chains(30.0, 2);
        assert_eq!(relaxed.positions.len(), mesh.positions.len());
        assert_eq!(relaxed.triangle_count(), mesh.triangle_count());
        assert!(relaxed.stats().is_closed);
        assert!(
            relaxed
                .positions
                .iter()
                .all(|position| position.iter().all(|value| value.is_finite()))
        );
    }

    #[test]
    fn clustered_simplify_reduces_triangle_count() {
        let graph = crate::presets::sphere();
        let mesh = graph.sample_grid(32).unwrap().surface_nets().unwrap();
        let simplified = mesh.simplify_clustered(0.5);
        assert!(simplified.triangle_count() > 0);
        assert!(simplified.triangle_count() < mesh.triangle_count());
        assert!(!simplified.positions.is_empty());
        assert!(
            simplified
                .positions
                .iter()
                .all(|position| position.iter().all(|value| value.is_finite()))
        );
    }

    #[test]
    fn sphere_mesh_is_closed_and_outward() {
        let graph = sphere_graph(1.0);
        let grid = graph.sample_grid(32).unwrap();
        let mesh = grid.surface_nets().unwrap();
        let stats = mesh.stats();

        assert!(stats.vertices > 0);
        assert!(stats.triangles > 0);
        assert_eq!(stats.boundary_edges, 0);
        assert_eq!(stats.non_manifold_edges, 0);
        assert!(stats.is_closed);
        assert!(stats.signed_volume > 0.0);

        for position in &mesh.positions {
            let radius = Vec3::from_array(*position).length();
            assert!((radius - 1.0).abs() < 0.1, "radius {radius}");
        }
    }

    #[test]
    fn empty_field_is_rejected() {
        let graph = FieldGraph::new();
        assert!(matches!(
            graph.sample_grid(16),
            Err(KernelError::InvalidBounds)
        ));
    }

    #[test]
    fn resolution_bounds_are_checked() {
        let graph = sphere_graph(1.0);
        assert!(matches!(
            graph.sample_grid(1),
            Err(KernelError::InvalidResolution(1))
        ));
        assert!(matches!(
            graph.sample_grid(2048),
            Err(KernelError::InvalidResolution(2048))
        ));
    }
}
