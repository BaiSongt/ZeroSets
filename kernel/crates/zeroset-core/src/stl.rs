use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use glam::Vec3;

use crate::Mesh;

/// Write a binary STL. Header is 80 bytes, then `u32` triangle count, then
/// 50 bytes per triangle (normal, three vertices, `u16` attribute).
pub fn write_stl_binary<W: Write>(mesh: &Mesh, writer: &mut W) -> std::io::Result<()> {
    let mut header = [0u8; 80];
    let label = b"ZeroSet binary STL";
    header[..label.len()].copy_from_slice(label);
    writer.write_all(&header)?;

    let triangle_count = mesh.triangle_count() as u32;
    writer.write_all(&triangle_count.to_le_bytes())?;

    for triangle in mesh.indices.chunks_exact(3) {
        let a = Vec3::from_array(mesh.positions[triangle[0] as usize]);
        let b = Vec3::from_array(mesh.positions[triangle[1] as usize]);
        let c = Vec3::from_array(mesh.positions[triangle[2] as usize]);
        let normal = (b - a).cross(c - a).normalize_or_zero();

        for vector in [normal, a, b, c] {
            writer.write_all(&vector.x.to_le_bytes())?;
            writer.write_all(&vector.y.to_le_bytes())?;
            writer.write_all(&vector.z.to_le_bytes())?;
        }
        writer.write_all(&0u16.to_le_bytes())?;
    }

    Ok(())
}

pub fn write_stl_binary_file(path: impl AsRef<Path>, mesh: &Mesh) -> std::io::Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    write_stl_binary(mesh, &mut writer)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FieldGraph, Vec3};

    #[test]
    fn binary_stl_length_matches_triangle_count() {
        let mut graph = FieldGraph::new();
        let sphere = graph.sphere(Vec3::ZERO, 1.0);
        graph.set_root(sphere);
        let grid = graph.sample_grid(16).unwrap();
        let mesh = grid.surface_nets().unwrap();

        let mut buffer = Vec::new();
        write_stl_binary(&mesh, &mut buffer).unwrap();
        assert_eq!(buffer.len(), 84 + mesh.triangle_count() * 50);
        assert_eq!(
            u32::from_le_bytes(buffer[80..84].try_into().unwrap()) as usize,
            mesh.triangle_count()
        );
    }
}
