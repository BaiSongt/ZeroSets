use std::sync::Arc;

use glam::Vec3;
use serde::{Deserialize, Serialize};
use zeroset_core::{Aabb, FieldCategory, Mesh, NodeId};

use crate::unit::{Quantity, Unit, UnitKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Field,
    Mesh,
    Number,
    Vector,
    Boolean,
    Text,
    List,
    Any,
}

impl ValueType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Field => "Field",
            Self::Mesh => "Mesh",
            Self::Number => "Number",
            Self::Vector => "Vector",
            Self::Boolean => "Boolean",
            Self::Text => "Text",
            Self::List => "List",
            Self::Any => "Any",
        }
    }

    pub fn accepts(self, actual: ValueType) -> bool {
        self == Self::Any || self == actual
    }
}

impl std::fmt::Display for ValueType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FieldHandle {
    pub node: NodeId,
    pub bounds: Aabb,
    pub category: FieldCategory,
}

#[derive(Clone, Debug)]
pub enum Value {
    Field(FieldHandle),
    Mesh(Arc<Mesh>),
    Number(Quantity),
    Vector(Vec3, Unit),
    Boolean(bool),
    Text(String),
    List(Vec<Value>),
}

impl Value {
    pub fn value_type(&self) -> ValueType {
        match self {
            Self::Field(_) => ValueType::Field,
            Self::Mesh(_) => ValueType::Mesh,
            Self::Number(_) => ValueType::Number,
            Self::Vector(_, _) => ValueType::Vector,
            Self::Boolean(_) => ValueType::Boolean,
            Self::Text(_) => ValueType::Text,
            Self::List(_) => ValueType::List,
        }
    }

    pub fn as_field(&self) -> Option<FieldHandle> {
        match self {
            Self::Field(handle) => Some(*handle),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<Quantity> {
        match self {
            Self::Number(quantity) => Some(*quantity),
            _ => None,
        }
    }

    pub fn as_vector(&self) -> Option<(Vec3, Unit)> {
        match self {
            Self::Vector(vector, unit) => Some((*vector, *unit)),
            _ => None,
        }
    }

    pub fn unit_kind(&self) -> Option<UnitKind> {
        match self {
            Self::Number(quantity) => Some(quantity.unit.kind()),
            Self::Vector(_, unit) => Some(unit.kind()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ParamValue {
    Number(Quantity),
    Vector(Vec3, Unit),
    Boolean(bool),
    Text(String),
}

impl ParamValue {
    pub fn number(value: f32, unit: Unit) -> Self {
        Self::Number(Quantity::new(value, unit))
    }

    pub fn vector(x: f32, y: f32, z: f32, unit: Unit) -> Self {
        Self::Vector(Vec3::new(x, y, z), unit)
    }

    pub fn value_type(&self) -> ValueType {
        match self {
            Self::Number(_) => ValueType::Number,
            Self::Vector(_, _) => ValueType::Vector,
            Self::Boolean(_) => ValueType::Boolean,
            Self::Text(_) => ValueType::Text,
        }
    }

    pub fn unit_kind(&self) -> Option<UnitKind> {
        match self {
            Self::Number(quantity) => Some(quantity.unit.kind()),
            Self::Vector(_, unit) => Some(unit.kind()),
            _ => None,
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            Self::Number(quantity) => Value::Number(*quantity),
            Self::Vector(vector, unit) => Value::Vector(*vector, *unit),
            Self::Boolean(value) => Value::Boolean(*value),
            Self::Text(value) => Value::Text(value.clone()),
        }
    }
}
