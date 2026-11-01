//! ZeroSet implicit geometry kernel.
//!
//! The kernel is deliberately small and layered:
//!
//! ```text
//! FieldGraph (arena DAG)
//!   -> evaluation (CPU today, WGSL codegen later)
//!   -> dense sampling
//!   -> Surface Nets extraction
//!   -> mesh diagnostics / STL
//! ```
//!
//! Precision policy for this first slice: `f32` everywhere, so CPU and GPU
//! results are directly comparable. An `f64` CPU path can be added later behind
//! the same graph without changing the IR.

pub mod aabb;
pub mod field;
pub mod mesh;
pub mod presets;
pub mod stl;

pub use aabb::Aabb;
pub use field::{FieldCategory, FieldGraph, Node, NodeId, Transform, category_name};
pub use glam::{Mat3, Quat, Vec2, Vec3};
pub use mesh::{
    BoundaryCapReport, GridSamples, GridSamplesF64, HausdorffBounds, Mesh, MeshF64, MeshStats,
    encode_packed_mesh,
};

/// Errors that can cross the public kernel API.
///
/// The variants are intentionally explicit: callers (CLI, napi, wasm) need to
/// distinguish "empty graph" from "bad resolution" from "unbounded field".
#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("field graph has no root node")]
    EmptyGraph,

    #[error("field graph references invalid node id {0}")]
    InvalidNode(u32),

    #[error("resolution must be at least 2, got {0}")]
    InvalidResolution(u32),

    #[error("field bounds are empty or not finite")]
    InvalidBounds,

    #[error("sample grid is too large; lower the resolution")]
    GridTooLarge,

    #[error("sample grid needs at least 2x2x2 corner samples")]
    GridTooSmall,

    #[error("transform scale must be finite and positive, got {0}")]
    InvalidScale(f32),

    #[error("transform is not finite or has a zero rotation")]
    InvalidTransform,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
