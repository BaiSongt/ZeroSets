//! nTop-style block registry and notebook graph.
//!
//! This crate is the layer above `zeroset-core`: blocks have typed inputs,
//! units, overloads, status, cached outputs, and dirty propagation. Field
//! outputs are node ids into a shared [`zeroset_core::FieldGraph`], so composing
//! blocks does not clone subgraphs.

pub mod builtin;
mod custom;
pub mod notebook;
pub mod registry;
pub mod unit;
pub mod value;

pub use custom::{CustomTemplate, custom_templates};
pub use notebook::{
    BlockInstance, BlockInstanceSnapshot, BlockStatus, Diagnostic, InstanceId, Notebook,
    NotebookError, NotebookSnapshot, SNAPSHOT_VERSION, Severity, UpdatePolicy,
};
pub use registry::{BlockCategory, BlockDef, BlockRegistry, InputDef, Overload};
pub use unit::{Quantity, Unit, UnitKind};
pub use value::{FieldHandle, ParamValue, Value, ValueType};
