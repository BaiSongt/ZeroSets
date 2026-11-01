use crate::registry::{BlockCategory, BlockDef, BlockRegistry, InputDef, Overload};
use crate::unit::{Unit, UnitKind};
use crate::value::{ParamValue, ValueType};

fn center() -> InputDef {
    InputDef::new("center", ValueType::Vector, false, UnitKind::Length)
        .with_default(ParamValue::vector(0.0, 0.0, 0.0, Unit::Millimeter))
}

fn number_input(name: &str, unit: UnitKind, default: Option<ParamValue>) -> InputDef {
    let mut input = InputDef::new(name, ValueType::Number, default.is_none(), unit);
    if let Some(value) = default {
        input = input.with_default(value);
    }
    input
}

fn vector_input(name: &str, unit: UnitKind, default: ParamValue) -> InputDef {
    InputDef::new(name, ValueType::Vector, false, unit).with_default(default)
}

fn field_input(name: &str) -> InputDef {
    InputDef::new(name, ValueType::Field, true, UnitKind::Any)
}

fn optional_field_input(name: &str) -> InputDef {
    InputDef::new(name, ValueType::Field, false, UnitKind::Any)
}

fn boolean_input(name: &str, default: bool) -> InputDef {
    InputDef::new(name, ValueType::Boolean, false, UnitKind::Any)
        .with_default(ParamValue::Boolean(default))
}

fn sphere() -> BlockDef {
    BlockDef::new(
        "sphere",
        "Sphere",
        BlockCategory::Primitive,
        "Analytic sphere. Overloads accept radius or diameter.",
        vec![
            Overload::new(
                "radius",
                vec![
                    center(),
                    number_input(
                        "radius",
                        UnitKind::Length,
                        Some(ParamValue::number(10.0, Unit::Millimeter)),
                    ),
                ],
                ValueType::Field,
            ),
            Overload::new(
                "diameter",
                vec![
                    center(),
                    number_input(
                        "diameter",
                        UnitKind::Length,
                        Some(ParamValue::number(20.0, Unit::Millimeter)),
                    ),
                ],
                ValueType::Field,
            ),
        ],
    )
}

fn box_block() -> BlockDef {
    BlockDef::new(
        "box",
        "Box",
        BlockCategory::Primitive,
        "Rounded box defined by its half extents.",
        vec![Overload::new(
            "box",
            vec![
                center(),
                vector_input(
                    "half",
                    UnitKind::Length,
                    ParamValue::vector(10.0, 10.0, 10.0, Unit::Millimeter),
                ),
                number_input(
                    "rounding",
                    UnitKind::Length,
                    Some(ParamValue::number(0.0, Unit::Millimeter)),
                ),
            ],
            ValueType::Field,
        )],
    )
}

fn cylinder() -> BlockDef {
    BlockDef::new(
        "cylinder",
        "Cylinder",
        BlockCategory::Primitive,
        "Capped cylinder with an arbitrary axis.",
        vec![Overload::new(
            "cylinder",
            vec![
                center(),
                vector_input(
                    "axis",
                    UnitKind::Unitless,
                    ParamValue::vector(0.0, 0.0, 1.0, Unit::Unitless),
                ),
                number_input(
                    "radius",
                    UnitKind::Length,
                    Some(ParamValue::number(5.0, Unit::Millimeter)),
                ),
                number_input(
                    "half_height",
                    UnitKind::Length,
                    Some(ParamValue::number(10.0, Unit::Millimeter)),
                ),
            ],
            ValueType::Field,
        )],
    )
}

fn capsule() -> BlockDef {
    BlockDef::new(
        "capsule",
        "Capsule",
        BlockCategory::Primitive,
        "Capsule between two endpoints.",
        vec![Overload::new(
            "capsule",
            vec![
                vector_input(
                    "a",
                    UnitKind::Length,
                    ParamValue::vector(-10.0, 0.0, 0.0, Unit::Millimeter),
                ),
                vector_input(
                    "b",
                    UnitKind::Length,
                    ParamValue::vector(10.0, 0.0, 0.0, Unit::Millimeter),
                ),
                number_input(
                    "radius",
                    UnitKind::Length,
                    Some(ParamValue::number(3.0, Unit::Millimeter)),
                ),
            ],
            ValueType::Field,
        )],
    )
}

fn torus() -> BlockDef {
    BlockDef::new(
        "torus",
        "Torus",
        BlockCategory::Primitive,
        "Torus around an arbitrary normal.",
        vec![Overload::new(
            "torus",
            vec![
                center(),
                vector_input(
                    "normal",
                    UnitKind::Unitless,
                    ParamValue::vector(0.0, 0.0, 1.0, Unit::Unitless),
                ),
                number_input(
                    "major",
                    UnitKind::Length,
                    Some(ParamValue::number(10.0, Unit::Millimeter)),
                ),
                number_input(
                    "minor",
                    UnitKind::Length,
                    Some(ParamValue::number(3.0, Unit::Millimeter)),
                ),
            ],
            ValueType::Field,
        )],
    )
}

fn boolean_two(id: &str, name: &str, description: &str) -> BlockDef {
    BlockDef::new(
        id,
        name,
        BlockCategory::Boolean,
        description,
        vec![Overload::new(
            "two",
            vec![field_input("a"), field_input("b")],
            ValueType::Field,
        )],
    )
}

fn union() -> BlockDef {
    BlockDef::new(
        "union",
        "Union",
        BlockCategory::Boolean,
        "Hard union; accepts two or three fields.",
        vec![
            Overload::new(
                "two",
                vec![field_input("a"), field_input("b")],
                ValueType::Field,
            ),
            Overload::new(
                "three",
                vec![field_input("a"), field_input("b"), field_input("c")],
                ValueType::Field,
            ),
            Overload::new(
                "list",
                vec![InputDef::new("items", ValueType::List, true, UnitKind::Any)],
                ValueType::Field,
            ),
        ],
    )
}

fn custom_block() -> BlockDef {
    BlockDef::new(
        "custom",
        "Custom Field",
        BlockCategory::Utility,
        "Compile a small field expression; optional fields bind as $a-$d / $name, numbers as $scale/$offset, vector as $translation, boolean as $flag.",
        vec![Overload::new(
            "expression",
            vec![
                InputDef::new("expression", ValueType::Text, false, UnitKind::Any)
                    .with_default(ParamValue::Text("sphere(0,0,0,10)".to_string())),
                optional_field_input("a"),
                optional_field_input("b"),
                optional_field_input("c"),
                optional_field_input("d"),
                InputDef::new("scale", ValueType::Number, false, UnitKind::Unitless)
                    .with_default(ParamValue::number(1.0, Unit::Unitless)),
                InputDef::new("offset", ValueType::Number, false, UnitKind::Length)
                    .with_default(ParamValue::number(0.0, Unit::Millimeter)),
                InputDef::new("translation", ValueType::Vector, false, UnitKind::Length)
                    .with_default(ParamValue::vector(0.0, 0.0, 0.0, Unit::Millimeter)),
                InputDef::new("flag", ValueType::Boolean, false, UnitKind::Any)
                    .with_default(ParamValue::Boolean(false)),
            ],
            ValueType::Field,
        )],
    )
}

fn list_block() -> BlockDef {
    BlockDef::new(
        "list",
        "Field List",
        BlockCategory::Utility,
        "Collects optional field inputs into a list for list-aware blocks.",
        vec![
            Overload::new(
                "fields",
                vec![
                    optional_field_input("a"),
                    optional_field_input("b"),
                    optional_field_input("c"),
                    optional_field_input("d"),
                ],
                ValueType::List,
            ),
            Overload::new(
                "two",
                vec![optional_field_input("a"), optional_field_input("b")],
                ValueType::List,
            ),
            Overload::new(
                "eight",
                vec![
                    optional_field_input("a"),
                    optional_field_input("b"),
                    optional_field_input("c"),
                    optional_field_input("d"),
                    optional_field_input("e"),
                    optional_field_input("f"),
                    optional_field_input("g"),
                    optional_field_input("h"),
                ],
                ValueType::List,
            ),
        ],
    )
}

fn smooth_boolean(id: &str, name: &str, description: &str) -> BlockDef {
    BlockDef::new(
        id,
        name,
        BlockCategory::Boolean,
        description,
        vec![Overload::new(
            "two",
            vec![
                field_input("a"),
                field_input("b"),
                number_input(
                    "k",
                    UnitKind::Length,
                    Some(ParamValue::number(1.0, Unit::Millimeter)),
                ),
            ],
            ValueType::Field,
        )],
    )
}

fn smooth_union() -> BlockDef {
    smooth_boolean("smooth_union", "Smooth Union", "Polynomial smooth union.")
}

fn offset() -> BlockDef {
    BlockDef::new(
        "offset",
        "Offset",
        BlockCategory::Transform,
        "Positive distance grows the solid, negative erodes it.",
        vec![Overload::new(
            "offset",
            vec![
                field_input("field"),
                number_input(
                    "distance",
                    UnitKind::Length,
                    Some(ParamValue::number(0.0, Unit::Millimeter)),
                ),
            ],
            ValueType::Field,
        )],
    )
}

fn transform() -> BlockDef {
    BlockDef::new(
        "transform",
        "Transform",
        BlockCategory::Transform,
        "Translation, axis-angle rotation, and uniform scale.",
        vec![Overload::new(
            "rigid",
            vec![
                field_input("field"),
                vector_input(
                    "translation",
                    UnitKind::Length,
                    ParamValue::vector(0.0, 0.0, 0.0, Unit::Millimeter),
                ),
                vector_input(
                    "axis",
                    UnitKind::Unitless,
                    ParamValue::vector(0.0, 0.0, 1.0, Unit::Unitless),
                ),
                number_input(
                    "angle",
                    UnitKind::Angle,
                    Some(ParamValue::number(0.0, Unit::Degree)),
                ),
                number_input(
                    "scale",
                    UnitKind::Unitless,
                    Some(ParamValue::number(1.0, Unit::Unitless)),
                ),
            ],
            ValueType::Field,
        )],
    )
}

fn mesh_inputs() -> Vec<InputDef> {
    vec![
        field_input("field"),
        number_input(
            "resolution",
            UnitKind::Unitless,
            Some(ParamValue::number(64.0, Unit::Unitless)),
        ),
        number_input(
            "crease",
            UnitKind::Angle,
            Some(ParamValue::number(0.0, Unit::Degree)),
        ),
        number_input(
            "simplify",
            UnitKind::Unitless,
            Some(ParamValue::number(1.0, Unit::Unitless)),
        ),
        number_input(
            "simplify_angle",
            UnitKind::Angle,
            Some(ParamValue::number(30.0, Unit::Degree)),
        ),
        number_input(
            "feature_relax",
            UnitKind::Unitless,
            Some(ParamValue::number(0.0, Unit::Unitless)),
        ),
        number_input(
            "feature_angle",
            UnitKind::Angle,
            Some(ParamValue::number(30.0, Unit::Degree)),
        ),
        boolean_input("cap", false),
    ]
}

fn mesh() -> BlockDef {
    BlockDef::new(
        "mesh",
        "Mesh from Field",
        BlockCategory::Mesh,
        "Surface Nets or Dual Contouring extraction at a dense grid resolution.",
        vec![
            Overload::new("dense", mesh_inputs(), ValueType::Mesh),
            Overload::new("dual_contour", mesh_inputs(), ValueType::Mesh),
        ],
    )
}

fn number() -> BlockDef {
    BlockDef::new(
        "number",
        "Number",
        BlockCategory::Input,
        "A typed number literal that can be linked into Number inputs.",
        vec![Overload::new(
            "value",
            vec![
                InputDef::new("value", ValueType::Number, false, UnitKind::Any)
                    .with_default(ParamValue::number(1.0, Unit::Unitless)),
            ],
            ValueType::Number,
        )],
    )
}

fn vector() -> BlockDef {
    BlockDef::new(
        "vector",
        "Vector",
        BlockCategory::Input,
        "A typed vector literal that can be linked into Vector inputs.",
        vec![Overload::new(
            "value",
            vec![
                InputDef::new("value", ValueType::Vector, false, UnitKind::Any)
                    .with_default(ParamValue::vector(0.0, 0.0, 0.0, Unit::Unitless)),
            ],
            ValueType::Vector,
        )],
    )
}

pub fn registry() -> BlockRegistry {
    let mut registry = BlockRegistry::new();
    registry.register(sphere());
    registry.register(box_block());
    registry.register(cylinder());
    registry.register(capsule());
    registry.register(torus());
    registry.register(union());
    registry.register(list_block());
    registry.register(boolean_two(
        "intersect",
        "Intersect",
        "Hard intersection of two fields.",
    ));
    registry.register(boolean_two(
        "subtract",
        "Subtract",
        "Hard difference: a minus b.",
    ));
    registry.register(smooth_union());
    registry.register(smooth_boolean(
        "smooth_intersect",
        "Smooth Intersect",
        "Polynomial smooth intersection.",
    ));
    registry.register(smooth_boolean(
        "smooth_subtract",
        "Smooth Subtract",
        "Polynomial smooth difference.",
    ));
    registry.register(offset());
    registry.register(transform());
    registry.register(custom_block());
    registry.register(mesh());
    registry.register(number());
    registry.register(vector());
    registry
}
