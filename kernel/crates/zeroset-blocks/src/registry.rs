use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::unit::UnitKind;
use crate::value::{ParamValue, ValueType};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockCategory {
    Primitive,
    Boolean,
    Transform,
    Mesh,
    Input,
    Utility,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InputDef {
    pub name: String,
    pub value_type: ValueType,
    pub required: bool,
    pub unit: UnitKind,
    pub default: Option<ParamValue>,
}

impl InputDef {
    pub fn new(
        name: impl Into<String>,
        value_type: ValueType,
        required: bool,
        unit: UnitKind,
    ) -> Self {
        Self {
            name: name.into(),
            value_type,
            required,
            unit,
            default: None,
        }
    }

    pub fn with_default(mut self, value: ParamValue) -> Self {
        self.default = Some(value);
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Overload {
    pub name: String,
    pub inputs: Vec<InputDef>,
    pub output: ValueType,
}

impl Overload {
    pub fn new(name: impl Into<String>, inputs: Vec<InputDef>, output: ValueType) -> Self {
        Self {
            name: name.into(),
            inputs,
            output,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockDef {
    pub id: String,
    pub name: String,
    pub category: BlockCategory,
    pub description: String,
    pub overloads: Vec<Overload>,
}

impl BlockDef {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        category: BlockCategory,
        description: impl Into<String>,
        overloads: Vec<Overload>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            category,
            description: description.into(),
            overloads,
        }
    }

    pub fn overload(&self, name: &str) -> Option<&Overload> {
        self.overloads.iter().find(|overload| overload.name == name)
    }
}

#[derive(Clone, Debug, Default)]
pub struct BlockRegistry {
    defs: Vec<BlockDef>,
    index: HashMap<String, usize>,
}

impl BlockRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn builtin() -> Self {
        crate::builtin::registry()
    }

    pub fn register(&mut self, def: BlockDef) {
        let index = self.defs.len();
        self.index.insert(def.id.clone(), index);
        self.defs.push(def);
    }

    pub fn get(&self, id: &str) -> Option<&BlockDef> {
        self.index.get(id).map(|index| &self.defs[*index])
    }

    pub fn defs(&self) -> &[BlockDef] {
        &self.defs
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }
}
