//! Units and quantities.
//!
//! The kernel's base length is the millimetre and its base angle is the radian.
//! Block inputs carry their own unit; evaluation converts to the base before it
//! reaches `FieldGraph`.

use std::f32::consts::PI;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Unitless,
    Millimeter,
    Centimeter,
    Meter,
    Inch,
    Radian,
    Degree,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    Any,
    Length,
    Angle,
    Unitless,
}

impl Unit {
    pub fn kind(self) -> UnitKind {
        match self {
            Self::Unitless => UnitKind::Unitless,
            Self::Millimeter | Self::Centimeter | Self::Meter | Self::Inch => UnitKind::Length,
            Self::Radian | Self::Degree => UnitKind::Angle,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Unitless => "",
            Self::Millimeter => "mm",
            Self::Centimeter => "cm",
            Self::Meter => "m",
            Self::Inch => "in",
            Self::Radian => "rad",
            Self::Degree => "deg",
        }
    }

    pub fn to_base(self, value: f32) -> f32 {
        match self {
            Self::Unitless | Self::Millimeter | Self::Radian => value,
            Self::Centimeter => value * 10.0,
            Self::Meter => value * 1000.0,
            Self::Inch => value * 25.4,
            Self::Degree => value * PI / 180.0,
        }
    }

    pub fn from_base(self, value: f32) -> f32 {
        match self {
            Self::Unitless | Self::Millimeter | Self::Radian => value,
            Self::Centimeter => value / 10.0,
            Self::Meter => value / 1000.0,
            Self::Inch => value / 25.4,
            Self::Degree => value * 180.0 / PI,
        }
    }

    pub fn satisfies(self, required: UnitKind) -> bool {
        required == UnitKind::Any || self.kind() == required
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quantity {
    pub value: f32,
    pub unit: Unit,
}

impl Quantity {
    pub fn new(value: f32, unit: Unit) -> Self {
        Self { value, unit }
    }

    pub fn base(value: f32) -> Self {
        Self {
            value,
            unit: Unit::Unitless,
        }
    }

    pub fn to_base(self) -> f32 {
        self.unit.to_base(self.value)
    }

    pub fn to(self, unit: Unit) -> Option<f32> {
        if self.unit.kind() == unit.kind() {
            Some(unit.from_base(self.to_base()))
        } else {
            None
        }
    }

    pub fn is_finite(self) -> bool {
        self.value.is_finite()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_units_convert_to_millimetres() {
        assert!((Quantity::new(1.0, Unit::Inch).to_base() - 25.4).abs() < 1e-5);
        assert!((Quantity::new(2.0, Unit::Centimeter).to_base() - 20.0).abs() < 1e-5);
        assert!((Quantity::new(0.5, Unit::Meter).to_base() - 500.0).abs() < 1e-4);
    }

    #[test]
    fn angles_convert_to_radians() {
        assert!((Quantity::new(180.0, Unit::Degree).to_base() - PI).abs() < 1e-6);
    }

    #[test]
    fn incompatible_units_do_not_convert() {
        assert!(
            Quantity::new(1.0, Unit::Millimeter)
                .to(Unit::Degree)
                .is_none()
        );
    }
}
