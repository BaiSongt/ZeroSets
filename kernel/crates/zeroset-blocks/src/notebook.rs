use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};
use zeroset_core::{FieldGraph, KernelError, NodeId, Transform};

use crate::custom::{Binding, CustomDiagnostic};
use crate::registry::{BlockDef, BlockRegistry, InputDef, Overload};
use crate::unit::{Quantity, Unit, UnitKind};
use crate::value::{FieldHandle, ParamValue, Value, ValueType};

/// Blocks that expose free-form field ports: any finite set of names is
/// accepted, and each linked port contributes one field. `list` collects
/// fields into a list; `custom` binds fields as `$name` variables.
fn has_dynamic_ports(def: &BlockDef) -> bool {
    def.id == "list" || def.id == "custom"
}

fn collect_field_roots(value: &Value, roots: &mut Vec<NodeId>) {
    match value {
        Value::Field(handle) => roots.push(handle.node),
        Value::List(items) => {
            for item in items {
                collect_field_roots(item, roots);
            }
        }
        _ => {}
    }
}

fn remap_field_handles(value: &mut Value, remap: &[Option<NodeId>]) {
    match value {
        Value::Field(handle) => {
            if let Some(Some(node)) = remap.get(handle.node.0 as usize) {
                handle.node = *node;
            }
        }
        Value::List(items) => {
            for item in items {
                remap_field_handles(item, remap);
            }
        }
        _ => {}
    }
}

pub type InstanceId = u64;

pub const SNAPSHOT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockStatus {
    Unbuilt,
    Built,
    Warning,
    Error,
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub input: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    /// Source line with its `N | ` gutter, for expression diagnostics.
    pub snippet: Option<String>,
    /// Instance that originally produced the diagnostic. Propagated errors keep
    /// the upstream source so a consumer can jump to the editable block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<InstanceId>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePolicy {
    #[default]
    Auto,
    Manual,
}

/// Serializable subset of a block instance: no compiled output, graph nodes,
/// status, or diagnostics. Reloading builds those from scratch.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockInstanceSnapshot {
    pub id: InstanceId,
    pub def_id: String,
    pub name: String,
    pub overload: Option<String>,
    pub params: BTreeMap<String, ParamValue>,
    pub inputs: BTreeMap<String, InstanceId>,
    pub enabled: bool,
    pub manual: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NotebookSnapshot {
    pub version: u32,
    pub next_id: InstanceId,
    #[serde(default)]
    pub update_policy: UpdatePolicy,
    pub instances: Vec<BlockInstanceSnapshot>,
}

#[derive(Clone, Debug)]
pub struct BlockInstance {
    pub id: InstanceId,
    pub def_id: String,
    pub name: String,
    pub overload: Option<String>,
    pub params: BTreeMap<String, ParamValue>,
    pub inputs: BTreeMap<String, InstanceId>,
    pub enabled: bool,
    pub manual: bool,
    pub status: BlockStatus,
    pub output: Option<Value>,
    pub diagnostics: Vec<Diagnostic>,
    pub dirty: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum NotebookError {
    #[error("unknown block instance {0}")]
    UnknownInstance(InstanceId),

    #[error("unknown block definition `{0}`")]
    UnknownBlock(String),

    #[error("block `{0}` has no overload `{1}`")]
    UnknownOverload(String, String),

    #[error("block instance {0} has no input `{1}`")]
    UnknownInput(InstanceId, String),

    #[error("block instance {0} is missing required input `{1}`")]
    MissingInput(InstanceId, String),

    #[error("block instance {instance} input `{input}` expects {expected}, got {actual}")]
    TypeMismatch {
        instance: InstanceId,
        input: String,
        expected: ValueType,
        actual: ValueType,
    },

    #[error(
        "block instance {instance} input `{input}` expects a {expected:?} unit, got {actual:?}"
    )]
    UnitMismatch {
        instance: InstanceId,
        input: String,
        expected: UnitKind,
        actual: Unit,
    },

    #[error("block instance {0} is disabled")]
    Disabled(InstanceId),

    #[error("cycle detected at block instance {0}")]
    Cycle(InstanceId),

    #[error("invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("invalid parameter: {diagnostic}")]
    CustomField {
        diagnostic: CustomDiagnostic,
        origin: InstanceId,
    },

    #[error("unsupported notebook snapshot version {0}")]
    UnsupportedSnapshotVersion(u32),

    #[error("duplicate block instance {0} in snapshot")]
    DuplicateInstance(InstanceId),

    #[error("invalid notebook snapshot: {0}")]
    InvalidSnapshot(String),

    #[error("kernel error: {0}")]
    Kernel(#[from] KernelError),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Clone, Debug, Default)]
pub struct Notebook {
    registry: BlockRegistry,
    graph: FieldGraph,
    instances: BTreeMap<InstanceId, BlockInstance>,
    next_id: InstanceId,
    update_policy: UpdatePolicy,
}

impl Notebook {
    pub fn new() -> Self {
        Self {
            registry: BlockRegistry::builtin(),
            graph: FieldGraph::new(),
            instances: BTreeMap::new(),
            next_id: 1,
            update_policy: UpdatePolicy::Auto,
        }
    }

    pub fn with_registry(registry: BlockRegistry) -> Self {
        Self {
            registry,
            ..Self::new()
        }
    }

    pub fn registry(&self) -> &BlockRegistry {
        &self.registry
    }

    pub fn update_policy(&self) -> UpdatePolicy {
        self.update_policy
    }

    pub fn set_update_policy(&mut self, policy: UpdatePolicy) {
        self.update_policy = policy;
    }

    /// The compiled arena. Field outputs are node ids into this graph.
    pub fn graph(&self) -> &FieldGraph {
        &self.graph
    }

    /// Drops graph nodes that are no longer reachable from any cached block
    /// output, remapping the surviving field handles in place.
    ///
    /// Block outputs and dirty flags are preserved, so building the same mesh
    /// again stays cached. Returns the number of nodes removed.
    pub fn compact(&mut self) -> usize {
        let mut roots = Vec::new();
        for instance in self.instances.values() {
            if let Some(value) = &instance.output {
                collect_field_roots(value, &mut roots);
            }
        }
        roots.sort_by_key(|node| node.0);
        roots.dedup();
        let old_len = self.graph.len();
        let (graph, remap) = self.graph.compacted(&roots);
        for instance in self.instances.values_mut() {
            if let Some(value) = &mut instance.output {
                remap_field_handles(value, &remap);
            }
        }
        self.graph = graph;
        old_len.saturating_sub(self.graph.len())
    }

    pub fn instances(&self) -> impl Iterator<Item = &BlockInstance> {
        self.instances.values()
    }

    /// Structural snapshot: ids, definitions, params, links, flags. Compiled
    /// outputs and graph nodes are intentionally excluded.
    pub fn snapshot(&self) -> NotebookSnapshot {
        NotebookSnapshot {
            version: SNAPSHOT_VERSION,
            next_id: self.next_id,
            update_policy: self.update_policy,
            instances: self
                .instances
                .values()
                .map(|instance| BlockInstanceSnapshot {
                    id: instance.id,
                    def_id: instance.def_id.clone(),
                    name: instance.name.clone(),
                    overload: instance.overload.clone(),
                    params: instance.params.clone(),
                    inputs: instance.inputs.clone(),
                    enabled: instance.enabled,
                    manual: instance.manual,
                })
                .collect(),
        }
    }

    pub fn from_snapshot(snapshot: NotebookSnapshot) -> Result<Self, NotebookError> {
        if snapshot.version != SNAPSHOT_VERSION {
            return Err(NotebookError::UnsupportedSnapshotVersion(snapshot.version));
        }

        let update_policy = snapshot.update_policy;
        let mut ids = HashSet::new();
        for instance in &snapshot.instances {
            if !ids.insert(instance.id) {
                return Err(NotebookError::DuplicateInstance(instance.id));
            }
        }

        let mut notebook = Self::new();
        let mut maximum_id = 0;
        for saved in snapshot.instances {
            maximum_id = maximum_id.max(saved.id);
            let def = notebook
                .registry
                .get(&saved.def_id)
                .cloned()
                .ok_or_else(|| NotebookError::UnknownBlock(saved.def_id.clone()))?;
            let overload = if let Some(name) = &saved.overload {
                def.overload(name)
                    .cloned()
                    .ok_or_else(|| NotebookError::UnknownOverload(def.id.clone(), name.clone()))?
            } else {
                def.overloads.first().cloned().ok_or_else(|| {
                    NotebookError::UnknownOverload(def.id.clone(), "<default>".to_string())
                })?
            };

            for (name, param) in &saved.params {
                let input = def
                    .overloads
                    .iter()
                    .flat_map(|overload| overload.inputs.iter())
                    .find(|input| input.name == *name)
                    .ok_or_else(|| NotebookError::UnknownInput(saved.id, name.clone()))?;
                if overload
                    .inputs
                    .iter()
                    .any(|candidate| candidate.name == *name)
                {
                    validate_input(saved.id, input, &param.to_value())?;
                }
            }
            for (name, source) in &saved.inputs {
                if !has_dynamic_ports(&def)
                    && !def
                        .overloads
                        .iter()
                        .any(|overload| overload.inputs.iter().any(|input| input.name == *name))
                {
                    return Err(NotebookError::UnknownInput(saved.id, name.clone()));
                }
                if !ids.contains(source) {
                    return Err(NotebookError::InvalidSnapshot(format!(
                        "instance {} references missing source {}",
                        saved.id, source
                    )));
                }
            }

            notebook.instances.insert(
                saved.id,
                BlockInstance {
                    id: saved.id,
                    def_id: saved.def_id,
                    name: saved.name,
                    overload: saved.overload,
                    params: saved.params,
                    inputs: saved.inputs,
                    enabled: saved.enabled,
                    manual: saved.manual,
                    status: BlockStatus::Unbuilt,
                    output: None,
                    diagnostics: Vec::new(),
                    dirty: true,
                },
            );
        }

        notebook.next_id = snapshot.next_id.max(maximum_id + 1).max(1);
        notebook.update_policy = update_policy;
        Ok(notebook)
    }

    pub fn to_json(&self) -> Result<String, NotebookError> {
        Ok(serde_json::to_string_pretty(&self.snapshot())?)
    }

    pub fn from_json(json: &str) -> Result<Self, NotebookError> {
        let snapshot: NotebookSnapshot = serde_json::from_str(json)?;
        Self::from_snapshot(snapshot)
    }

    pub fn instance(&self, id: InstanceId) -> Option<&BlockInstance> {
        self.instances.get(&id)
    }

    /// Number parameter converted to the kernel base unit.
    pub fn number_param(&self, id: InstanceId, name: &str) -> Option<f32> {
        match self.instance(id)?.params.get(name)? {
            ParamValue::Number(quantity) => Some(quantity.to_base()),
            _ => None,
        }
    }

    pub fn add_block(
        &mut self,
        def_id: &str,
        name: impl Into<String>,
    ) -> Result<InstanceId, NotebookError> {
        let def = self
            .registry
            .get(def_id)
            .ok_or_else(|| NotebookError::UnknownBlock(def_id.to_string()))?
            .clone();
        let overload = def
            .overloads
            .first()
            .ok_or_else(|| NotebookError::UnknownBlock(def_id.to_string()))?
            .clone();

        let id = self.next_id;
        self.next_id += 1;
        let mut params = BTreeMap::new();
        for input in &overload.inputs {
            if let Some(default) = &input.default {
                params.insert(input.name.clone(), default.clone());
            }
        }
        let instance = BlockInstance {
            id,
            def_id: def_id.to_string(),
            name: name.into(),
            overload: Some(overload.name.clone()),
            params,
            inputs: BTreeMap::new(),
            enabled: true,
            manual: false,
            status: BlockStatus::Unbuilt,
            output: None,
            diagnostics: Vec::new(),
            dirty: true,
        };
        self.instances.insert(id, instance);
        Ok(id)
    }

    pub fn set_overload(&mut self, id: InstanceId, overload: &str) -> Result<(), NotebookError> {
        let def = self.def_for_instance(id)?;
        let target = def
            .overload(overload)
            .ok_or_else(|| NotebookError::UnknownOverload(def.id.clone(), overload.to_string()))?
            .clone();
        {
            let instance = self
                .instances
                .get_mut(&id)
                .ok_or(NotebookError::UnknownInstance(id))?;
            instance.overload = Some(target.name.clone());
            for input in &target.inputs {
                if let Some(default) = &input.default {
                    instance
                        .params
                        .entry(input.name.clone())
                        .or_insert_with(|| default.clone());
                }
            }
        }
        self.mark_dirty(id);
        Ok(())
    }

    pub fn set_param(
        &mut self,
        id: InstanceId,
        name: &str,
        value: ParamValue,
    ) -> Result<(), NotebookError> {
        let def = self.def_for_instance(id)?;
        if !def
            .overloads
            .iter()
            .any(|overload| overload.inputs.iter().any(|input| input.name == name))
        {
            return Err(NotebookError::UnknownInput(id, name.to_string()));
        }
        let instance = self
            .instances
            .get_mut(&id)
            .ok_or(NotebookError::UnknownInstance(id))?;
        if instance.params.get(name) == Some(&value) {
            return Ok(());
        }
        instance.params.insert(name.to_string(), value);
        self.mark_dirty(id);
        Ok(())
    }

    pub fn set_input(
        &mut self,
        id: InstanceId,
        name: &str,
        source: InstanceId,
    ) -> Result<(), NotebookError> {
        if !self.instances.contains_key(&source) {
            return Err(NotebookError::UnknownInstance(source));
        }
        let def = self.def_for_instance(id)?;
        if !has_dynamic_ports(&def)
            && !def
                .overloads
                .iter()
                .any(|overload| overload.inputs.iter().any(|input| input.name == name))
        {
            return Err(NotebookError::UnknownInput(id, name.to_string()));
        }
        let instance = self
            .instances
            .get_mut(&id)
            .ok_or(NotebookError::UnknownInstance(id))?;
        instance.inputs.insert(name.to_string(), source);
        self.mark_dirty(id);
        Ok(())
    }

    pub fn clear_input(&mut self, id: InstanceId, name: &str) -> Result<(), NotebookError> {
        let instance = self
            .instances
            .get_mut(&id)
            .ok_or(NotebookError::UnknownInstance(id))?;
        instance.inputs.remove(name);
        self.mark_dirty(id);
        Ok(())
    }

    pub fn set_enabled(&mut self, id: InstanceId, enabled: bool) -> Result<(), NotebookError> {
        let instance = self
            .instances
            .get_mut(&id)
            .ok_or(NotebookError::UnknownInstance(id))?;
        instance.enabled = enabled;
        self.mark_dirty(id);
        Ok(())
    }

    pub fn set_manual(&mut self, id: InstanceId, manual: bool) -> Result<(), NotebookError> {
        let instance = self
            .instances
            .get_mut(&id)
            .ok_or(NotebookError::UnknownInstance(id))?;
        instance.manual = manual;
        Ok(())
    }

    pub fn rename(&mut self, id: InstanceId, name: impl Into<String>) -> Result<(), NotebookError> {
        let instance = self
            .instances
            .get_mut(&id)
            .ok_or(NotebookError::UnknownInstance(id))?;
        instance.name = name.into();
        Ok(())
    }

    pub fn remove_block(&mut self, id: InstanceId) -> Result<(), NotebookError> {
        if self.instances.remove(&id).is_none() {
            return Err(NotebookError::UnknownInstance(id));
        }
        let consumers: Vec<InstanceId> = self
            .instances
            .values_mut()
            .filter_map(|instance| {
                let had = instance.inputs.values().any(|source| *source == id);
                instance.inputs.retain(|_, source| *source != id);
                had.then_some(instance.id)
            })
            .collect();
        for consumer in consumers {
            self.mark_dirty(consumer);
        }
        Ok(())
    }

    /// Evaluate one block and all of its dependencies.
    pub fn evaluate(&mut self, id: InstanceId) -> Result<Value, NotebookError> {
        let result = self.evaluate_inner(id, &mut Vec::new());
        if let Ok(value) = &result {
            if let Some(instance) = self.instances.get_mut(&id) {
                instance.status = BlockStatus::Built;
                instance.output = Some(value.clone());
                instance.diagnostics.clear();
                instance.dirty = false;
            }
        }
        result
    }

    pub fn evaluate_all(&mut self) {
        let ids: Vec<InstanceId> = self.instances.keys().copied().collect();
        for id in ids {
            let _ = self.evaluate(id);
        }
    }

    fn evaluate_inner(
        &mut self,
        id: InstanceId,
        stack: &mut Vec<InstanceId>,
    ) -> Result<Value, NotebookError> {
        let result = self.evaluate_inner_body(id, stack);
        if let Err(error) = &result {
            self.apply_error(id, error);
        }
        result
    }

    fn evaluate_inner_body(
        &mut self,
        id: InstanceId,
        stack: &mut Vec<InstanceId>,
    ) -> Result<Value, NotebookError> {
        let instance = self
            .instances
            .get(&id)
            .cloned()
            .ok_or(NotebookError::UnknownInstance(id))?;

        if !instance.enabled {
            return Err(NotebookError::Disabled(id));
        }
        if !instance.dirty {
            if let Some(output) = &instance.output {
                return Ok(output.clone());
            }
        }
        if stack.contains(&id) {
            return Err(NotebookError::Cycle(id));
        }
        stack.push(id);

        let def = self
            .registry
            .get(&instance.def_id)
            .cloned()
            .ok_or_else(|| NotebookError::UnknownBlock(instance.def_id.clone()))?;
        let overload = self.resolve_overload(&def, &instance)?;
        let overload = overload.clone();

        let mut values = BTreeMap::new();
        for input in &overload.inputs {
            let value = if let Some(source) = instance.inputs.get(&input.name).copied() {
                let value = self.evaluate_inner(source, stack)?;
                validate_input(id, input, &value)?;
                value
            } else if let Some(param) = instance.params.get(&input.name) {
                let value = param.to_value();
                validate_input(id, input, &value)?;
                value
            } else if let Some(default) = &input.default {
                let value = default.to_value();
                validate_input(id, input, &value)?;
                value
            } else if input.required {
                return Err(NotebookError::MissingInput(id, input.name.clone()));
            } else {
                continue;
            };
            values.insert(input.name.clone(), value);
        }

        if has_dynamic_ports(&def) {
            // Blocks with free-form ports accept arbitrary port names. Ports
            // outside the selected overload are stored in `instance.inputs`
            // exactly like regular links, so snapshots need no separate list.
            let extras: Vec<(String, InstanceId)> = instance
                .inputs
                .iter()
                .filter(|(name, _)| !values.contains_key(*name))
                .map(|(name, source)| (name.clone(), *source))
                .collect();
            for (name, source) in extras {
                let value = self.evaluate_inner(source, stack)?;
                if value.value_type() != ValueType::Field {
                    return Err(NotebookError::TypeMismatch {
                        instance: id,
                        input: name,
                        expected: ValueType::Field,
                        actual: value.value_type(),
                    });
                }
                values.insert(name, value);
            }
        }

        let output = self.compute(id, &def, &overload, &values)?;
        stack.pop();

        if let Some(instance) = self.instances.get_mut(&id) {
            instance.status = BlockStatus::Built;
            instance.output = Some(output.clone());
            instance.diagnostics.clear();
            instance.dirty = false;
        }
        Ok(output)
    }

    fn apply_error(&mut self, id: InstanceId, error: &NotebookError) {
        let status = if matches!(error, NotebookError::Disabled(_)) {
            BlockStatus::Disabled
        } else {
            BlockStatus::Error
        };
        let input = match error {
            NotebookError::MissingInput(_, input)
            | NotebookError::TypeMismatch { input, .. }
            | NotebookError::UnitMismatch { input, .. } => Some(input.clone()),
            _ => None,
        };
        let (line, column, snippet) = match error {
            NotebookError::CustomField { diagnostic, .. } => (
                Some(diagnostic.line),
                Some(diagnostic.column),
                Some(diagnostic.snippet.clone()),
            ),
            _ => (None, None, None),
        };
        let source = match error {
            NotebookError::CustomField { origin, .. } => Some(*origin),
            _ => None,
        };
        if let Some(instance) = self.instances.get_mut(&id) {
            instance.status = status;
            instance.output = None;
            instance.diagnostics = vec![Diagnostic {
                severity: Severity::Error,
                message: error.to_string(),
                input,
                line,
                column,
                snippet,
                source,
            }];
        }
    }

    fn resolve_overload<'a>(
        &'a self,
        def: &'a BlockDef,
        instance: &BlockInstance,
    ) -> Result<&'a Overload, NotebookError> {
        if let Some(name) = &instance.overload {
            return def
                .overload(name)
                .ok_or_else(|| NotebookError::UnknownOverload(def.id.clone(), name.clone()));
        }
        def.overloads
            .first()
            .ok_or_else(|| NotebookError::UnknownOverload(def.id.clone(), "<default>".to_string()))
    }

    fn compute(
        &mut self,
        id: InstanceId,
        def: &BlockDef,
        overload: &Overload,
        values: &BTreeMap<String, Value>,
    ) -> Result<Value, NotebookError> {
        match def.id.as_str() {
            "number" | "vector" => values
                .get("value")
                .cloned()
                .ok_or(NotebookError::MissingInput(id, "value".to_string())),
            "sphere" => {
                let center = vector_base(values, "center")?;
                let radius = match overload.name.as_str() {
                    "diameter" => number_base(values, "diameter")? * 0.5,
                    _ => number_base(values, "radius")?,
                };
                let node = self.graph.sphere(center, radius);
                Ok(self.field_value(node))
            }
            "box" => {
                let node = self.graph.rounded_box(
                    vector_base(values, "center")?,
                    vector_base(values, "half")?,
                    number_base(values, "rounding")?,
                );
                Ok(self.field_value(node))
            }
            "cylinder" => {
                let node = self.graph.cylinder(
                    vector_base(values, "center")?,
                    vector_base(values, "axis")?,
                    number_base(values, "radius")?,
                    number_base(values, "half_height")?,
                );
                Ok(self.field_value(node))
            }
            "capsule" => {
                let node = self.graph.capsule(
                    vector_base(values, "a")?,
                    vector_base(values, "b")?,
                    number_base(values, "radius")?,
                );
                Ok(self.field_value(node))
            }
            "torus" => {
                let node = self.graph.torus(
                    vector_base(values, "center")?,
                    vector_base(values, "normal")?,
                    number_base(values, "major")?,
                    number_base(values, "minor")?,
                );
                Ok(self.field_value(node))
            }
            "union" | "intersect" | "subtract" => {
                if def.id == "union" && overload.name == "list" {
                    let items = match values.get("items") {
                        Some(Value::List(items)) => items,
                        _ => {
                            return Err(NotebookError::InvalidParameter(
                                "union list requires a List input".to_string(),
                            ));
                        }
                    };
                    let mut nodes = items
                        .iter()
                        .filter_map(Value::as_field)
                        .map(|handle| handle.node);
                    let first = nodes.next().ok_or_else(|| {
                        NotebookError::InvalidParameter("union list is empty".to_string())
                    })?;
                    let mut root = first;
                    for node in nodes {
                        root = self.graph.union(root, node);
                    }
                    return Ok(self.field_value(root));
                }
                let a = field_node(values, "a")?;
                let b = field_node(values, "b")?;
                let node = match def.id.as_str() {
                    "union" => {
                        if overload.name == "three" {
                            let c = field_node(values, "c")?;
                            let ab = self.graph.union(a, b);
                            self.graph.union(ab, c)
                        } else {
                            self.graph.union(a, b)
                        }
                    }
                    "intersect" => self.graph.intersect(a, b),
                    _ => self.graph.subtract(a, b),
                };
                Ok(self.field_value(node))
            }
            "list" => {
                let items = values
                    .values()
                    .filter(|value| value.value_type() == ValueType::Field)
                    .cloned()
                    .collect();
                Ok(Value::List(items))
            }
            "custom" => {
                let expression = text_value(values, "expression")?;
                let bindings: Vec<(String, Binding)> = values
                    .iter()
                    .filter_map(|(name, value)| {
                        let binding = match value {
                            Value::Field(handle) => Binding::Field(handle.node),
                            Value::Number(quantity) => Binding::Number(quantity.to_base()),
                            Value::Vector(vector, unit) => {
                                Binding::Vector(*vector * unit.to_base(1.0))
                            }
                            Value::Boolean(value) => Binding::Boolean(*value),
                            _ => return None,
                        };
                        Some((name.clone(), binding))
                    })
                    .collect();
                let node = match crate::custom::parse_with_bindings(
                    &mut self.graph,
                    expression,
                    &bindings,
                ) {
                    Ok(node) => node,
                    Err(error) => {
                        return Err(match CustomDiagnostic::from_message(&error) {
                            Some(diagnostic) => NotebookError::CustomField {
                                diagnostic,
                                origin: id,
                            },
                            None => NotebookError::InvalidParameter(error),
                        });
                    }
                };
                Ok(self.field_value(node))
            }
            "smooth_union" | "smooth_intersect" | "smooth_subtract" => {
                let a = field_node(values, "a")?;
                let b = field_node(values, "b")?;
                let k = number_base(values, "k")?;
                let node = match def.id.as_str() {
                    "smooth_union" => self.graph.smooth_union(a, b, k),
                    "smooth_intersect" => self.graph.smooth_intersect(a, b, k),
                    _ => self.graph.smooth_subtract(a, b, k),
                };
                Ok(self.field_value(node))
            }
            "offset" => {
                let field = field_node(values, "field")?;
                let node = self.graph.offset(field, number_base(values, "distance")?);
                Ok(self.field_value(node))
            }
            "transform" => {
                let field = field_node(values, "field")?;
                let translation = vector_base(values, "translation")?;
                let axis = vector_base(values, "axis")?;
                let angle = number_base(values, "angle")?;
                let scale = number_base(values, "scale")?;
                if axis.length_squared() < 1e-12 {
                    return Err(NotebookError::InvalidParameter(
                        "transform axis must be non-zero".to_string(),
                    ));
                }
                let transform = Transform::new(
                    translation,
                    Quat::from_axis_angle(axis.normalize(), angle),
                    scale,
                )?;
                let node = self.graph.transform(field, transform);
                Ok(self.field_value(node))
            }
            "mesh" => {
                let field = field_node(values, "field")?;
                let resolution = number_base(values, "resolution")?.round() as i64;
                if !(2..=512).contains(&resolution) {
                    return Err(NotebookError::InvalidParameter(format!(
                        "mesh resolution must be in [2, 512], got {resolution}"
                    )));
                }
                let grid = self.graph.sample_grid_at(field, resolution as u32)?;
                let mesh = if overload.name == "dual_contour" {
                    grid.dual_contour()?
                } else {
                    grid.surface_nets()?
                };
                let crease = number_base(values, "crease").unwrap_or(0.0).to_degrees();
                let simplify = number_base(values, "simplify").unwrap_or(1.0);
                let simplify_angle = number_base(values, "simplify_angle")
                    .unwrap_or(30.0f32.to_radians())
                    .to_degrees();
                let mesh = if simplify < 0.999 {
                    mesh.simplify_edge_collapse_with_angle(simplify, simplify_angle)
                } else {
                    mesh
                };
                let feature_relax = number_base(values, "feature_relax")
                    .unwrap_or(0.0)
                    .round()
                    .clamp(0.0, 8.0) as u32;
                let feature_angle = number_base(values, "feature_angle")
                    .unwrap_or(30.0f32.to_radians())
                    .to_degrees();
                let mesh = if feature_relax > 0 {
                    mesh.relax_feature_chains(feature_angle, feature_relax)
                } else {
                    mesh
                };
                let cap = matches!(values.get("cap"), Some(Value::Boolean(true)));
                let mesh = if cap { mesh.cap_boundary_loops() } else { mesh };
                let mesh = if crease > 0.0 {
                    mesh.with_crease_normals(crease)
                } else {
                    mesh
                };
                Ok(Value::Mesh(Arc::new(mesh)))
            }
            other => Err(NotebookError::UnknownBlock(other.to_string())),
        }
    }

    fn field_value(&self, node: NodeId) -> Value {
        Value::Field(FieldHandle {
            node,
            bounds: self.graph.bounds_of(node),
            category: self.graph.category_of(node),
        })
    }

    fn def_for_instance(&self, id: InstanceId) -> Result<BlockDef, NotebookError> {
        let instance = self
            .instances
            .get(&id)
            .ok_or(NotebookError::UnknownInstance(id))?;
        self.registry
            .get(&instance.def_id)
            .cloned()
            .ok_or_else(|| NotebookError::UnknownBlock(instance.def_id.clone()))
    }

    fn mark_dirty(&mut self, id: InstanceId) {
        let mut visited = Vec::new();
        self.mark_dirty_inner(id, &mut visited);
    }

    fn mark_dirty_inner(&mut self, id: InstanceId, visited: &mut Vec<InstanceId>) {
        if visited.contains(&id) {
            return;
        }
        visited.push(id);
        if let Some(instance) = self.instances.get_mut(&id) {
            instance.dirty = true;
            instance.status = BlockStatus::Unbuilt;
        }
        let consumers: Vec<InstanceId> = self
            .instances
            .values()
            .filter(|instance| instance.inputs.values().any(|source| *source == id))
            .map(|instance| instance.id)
            .collect();
        for consumer in consumers {
            self.mark_dirty_inner(consumer, visited);
        }
    }
}

fn validate_input(
    instance: InstanceId,
    input: &InputDef,
    value: &Value,
) -> Result<(), NotebookError> {
    if !input.value_type.accepts(value.value_type()) {
        return Err(NotebookError::TypeMismatch {
            instance,
            input: input.name.clone(),
            expected: input.value_type,
            actual: value.value_type(),
        });
    }
    if input.unit != UnitKind::Any {
        match value {
            Value::Number(quantity) if !quantity.unit.satisfies(input.unit) => {
                return Err(NotebookError::UnitMismatch {
                    instance,
                    input: input.name.clone(),
                    expected: input.unit,
                    actual: quantity.unit,
                });
            }
            Value::Vector(_, unit) if !unit.satisfies(input.unit) => {
                return Err(NotebookError::UnitMismatch {
                    instance,
                    input: input.name.clone(),
                    expected: input.unit,
                    actual: *unit,
                });
            }
            _ => {}
        }
    }
    Ok(())
}

fn number_base(values: &BTreeMap<String, Value>, name: &str) -> Result<f32, NotebookError> {
    values
        .get(name)
        .and_then(Value::as_number)
        .map(Quantity::to_base)
        .ok_or_else(|| NotebookError::InvalidParameter(format!("missing number `{name}`")))
}

fn text_value<'a>(
    values: &'a BTreeMap<String, Value>,
    name: &str,
) -> Result<&'a str, NotebookError> {
    match values.get(name) {
        Some(Value::Text(text)) => Ok(text),
        _ => Err(NotebookError::InvalidParameter(format!(
            "missing text `{name}`"
        ))),
    }
}

fn vector_base(values: &BTreeMap<String, Value>, name: &str) -> Result<Vec3, NotebookError> {
    let (vector, unit) = values
        .get(name)
        .and_then(Value::as_vector)
        .ok_or_else(|| NotebookError::InvalidParameter(format!("missing vector `{name}`")))?;
    Ok(vector * unit.to_base(1.0))
}

fn field_node(values: &BTreeMap<String, Value>, name: &str) -> Result<NodeId, NotebookError> {
    values
        .get(name)
        .and_then(Value::as_field)
        .map(|handle| handle.node)
        .ok_or_else(|| NotebookError::InvalidParameter(format!("missing field `{name}`")))
}
