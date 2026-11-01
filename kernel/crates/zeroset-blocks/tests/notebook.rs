use std::sync::Arc;

use glam::Vec3;
use zeroset_blocks::{BlockStatus, InstanceId, Notebook, NotebookError, ParamValue, Unit, Value};

fn add_sphere(notebook: &mut Notebook, radius_mm: f32) -> InstanceId {
    let id = notebook.add_block("sphere", "Sphere").unwrap();
    notebook
        .set_param(
            id,
            "radius",
            ParamValue::number(radius_mm, Unit::Millimeter),
        )
        .unwrap();
    id
}

fn add_mesh(notebook: &mut Notebook, field: InstanceId, resolution: f32) -> InstanceId {
    let id = notebook.add_block("mesh", "Mesh").unwrap();
    notebook.set_input(id, "field", field).unwrap();
    notebook
        .set_param(
            id,
            "resolution",
            ParamValue::number(resolution, Unit::Unitless),
        )
        .unwrap();
    id
}

fn mesh_value(notebook: &mut Notebook, id: InstanceId) -> Arc<zeroset_core::Mesh> {
    match notebook.evaluate(id).unwrap() {
        Value::Mesh(mesh) => mesh,
        other => panic!("expected mesh, got {:?}", other.value_type()),
    }
}

#[test]
fn number_param_converts_to_kernel_base() {
    let mut notebook = Notebook::new();
    let mesh = notebook.add_block("mesh", "Mesh").unwrap();
    notebook
        .set_param(
            mesh,
            "feature_angle",
            ParamValue::number(45.0, Unit::Degree),
        )
        .unwrap();
    let value = notebook.number_param(mesh, "feature_angle").unwrap();
    assert!((value - 45.0f32.to_radians()).abs() < 1e-6);
    assert!(notebook.number_param(mesh, "missing").is_none());
}

#[test]
fn sphere_mesh_is_cached_until_input_changes() {
    let mut notebook = Notebook::new();
    let sphere = add_sphere(&mut notebook, 10.0);
    let mesh = add_mesh(&mut notebook, sphere, 24.0);

    let first = mesh_value(&mut notebook, mesh);
    assert!(first.triangle_count() > 0);

    let second = mesh_value(&mut notebook, mesh);
    assert!(
        Arc::ptr_eq(&first, &second),
        "unchanged mesh should be cached"
    );

    notebook
        .set_param(sphere, "radius", ParamValue::number(20.0, Unit::Millimeter))
        .unwrap();
    assert!(notebook.instance(sphere).unwrap().dirty);
    assert!(notebook.instance(mesh).unwrap().dirty);

    let third = mesh_value(&mut notebook, mesh);
    assert!(
        !Arc::ptr_eq(&first, &third),
        "changed input must rebuild mesh"
    );
    assert!(third.triangle_count() > 0);
}

#[test]
fn compact_drops_orphaned_graph_nodes_and_keeps_mesh_cached() {
    let mut notebook = Notebook::new();
    let sphere = add_sphere(&mut notebook, 10.0);
    let mesh = add_mesh(&mut notebook, sphere, 24.0);

    let first = mesh_value(&mut notebook, mesh);
    assert!(first.triangle_count() > 0);
    let base_len = notebook.graph().len();

    for radius in [12.0, 14.0] {
        notebook
            .set_param(
                sphere,
                "radius",
                ParamValue::number(radius, Unit::Millimeter),
            )
            .unwrap();
        mesh_value(&mut notebook, mesh);
    }
    let grown = notebook.graph().len();
    assert!(grown > base_len, "re-evaluation should leave orphan nodes");

    let before = mesh_value(&mut notebook, mesh);
    let before_len = notebook.graph().len();
    let removed = notebook.compact();
    assert!(removed > 0, "orphaned nodes should be removed");
    assert_eq!(before_len - notebook.graph().len(), removed);
    assert_eq!(notebook.compact(), 0, "compaction must be idempotent");

    let after = mesh_value(&mut notebook, mesh);
    assert!(
        Arc::ptr_eq(&before, &after),
        "compaction must not invalidate cached mesh output"
    );
    assert_eq!(notebook.graph().len(), before_len - removed);

    notebook
        .set_param(sphere, "radius", ParamValue::number(16.0, Unit::Millimeter))
        .unwrap();
    assert!(mesh_value(&mut notebook, mesh).triangle_count() > 0);
}

#[test]
fn compact_remaps_field_handles_inside_list_output() {
    let mut notebook = Notebook::new();
    let first = add_sphere(&mut notebook, 10.0);
    let second = add_sphere(&mut notebook, 12.0);

    let list = notebook.add_block("list", "List").unwrap();
    notebook.set_input(list, "p0", first).unwrap();
    notebook.set_input(list, "p1", second).unwrap();

    let union = notebook.add_block("union", "Union").unwrap();
    notebook.set_overload(union, "list").unwrap();
    notebook.set_input(union, "items", list).unwrap();
    let mesh = add_mesh(&mut notebook, union, 24.0);

    let initial = mesh_value(&mut notebook, mesh);
    assert!(initial.triangle_count() > 0);

    notebook
        .set_param(first, "radius", ParamValue::number(14.0, Unit::Millimeter))
        .unwrap();
    let rebuilt = mesh_value(&mut notebook, mesh);
    assert!(rebuilt.triangle_count() > 0);

    let before_len = notebook.graph().len();
    let removed = notebook.compact();
    assert!(removed > 0);
    assert!(notebook.graph().len() < before_len);

    // The cached List output contains remapped field handles. A consumer
    // evaluated only after compaction must still see valid node ids.
    let consumer = notebook.add_block("union", "Union").unwrap();
    notebook.set_overload(consumer, "list").unwrap();
    notebook.set_input(consumer, "items", list).unwrap();
    let field = notebook.evaluate(consumer).unwrap();
    let handle = field.as_field().expect("union output is a field");
    let value = notebook.graph().evaluate_at(handle.node, Vec3::ZERO);
    assert!(value.is_finite());
}

#[test]
fn sphere_diameter_overload_changes_radius() {
    let mut notebook = Notebook::new();
    let sphere = notebook.add_block("sphere", "Sphere").unwrap();
    notebook.set_overload(sphere, "diameter").unwrap();
    notebook
        .set_param(
            sphere,
            "diameter",
            ParamValue::number(20.0, Unit::Millimeter),
        )
        .unwrap();

    let value = notebook.evaluate(sphere).unwrap();
    let handle = value.as_field().unwrap();
    let size = handle.bounds.max_extent();
    assert!(
        (size - 20.0).abs() < 1e-3,
        "diameter should give radius 10, size {size}"
    );
}

#[test]
fn unit_mismatch_is_reported_on_the_instance() {
    let mut notebook = Notebook::new();
    let sphere = add_sphere(&mut notebook, 10.0);
    let mesh = add_mesh(&mut notebook, sphere, 16.0);

    notebook
        .set_param(sphere, "radius", ParamValue::number(1.0, Unit::Radian))
        .unwrap();
    let error = notebook.evaluate(mesh).unwrap_err();
    assert!(matches!(error, NotebookError::UnitMismatch { .. }));
    assert_eq!(
        notebook.instance(sphere).unwrap().status,
        BlockStatus::Error
    );
    assert!(!notebook.instance(sphere).unwrap().diagnostics.is_empty());
}

#[test]
fn type_mismatch_and_missing_input_are_reported() {
    let mut notebook = Notebook::new();
    let number = notebook.add_block("number", "Number").unwrap();
    let union = notebook.add_block("union", "Union").unwrap();

    notebook.set_input(union, "a", number).unwrap();
    let error = notebook.evaluate(union).unwrap_err();
    assert!(matches!(error, NotebookError::TypeMismatch { .. }));

    let sphere = add_sphere(&mut notebook, 5.0);
    notebook.set_input(union, "a", sphere).unwrap();
    notebook.clear_input(union, "b").unwrap();
    let error = notebook.evaluate(union).unwrap_err();
    assert!(matches!(error, NotebookError::MissingInput(_, _)));
}

#[test]
fn cycles_are_detected() {
    let mut notebook = Notebook::new();
    let union = notebook.add_block("union", "Union").unwrap();
    notebook.set_input(union, "a", union).unwrap();

    let error = notebook.evaluate(union).unwrap_err();
    assert!(matches!(error, NotebookError::Cycle(_)));
}

#[test]
fn disabled_block_reports_disabled_status() {
    let mut notebook = Notebook::new();
    let sphere = add_sphere(&mut notebook, 10.0);
    notebook.set_enabled(sphere, false).unwrap();

    let error = notebook.evaluate(sphere).unwrap_err();
    assert!(matches!(error, NotebookError::Disabled(_)));
    assert_eq!(
        notebook.instance(sphere).unwrap().status,
        BlockStatus::Disabled
    );
}

#[test]
fn three_way_union_and_dirty_propagation() {
    let mut notebook = Notebook::new();
    let a = add_sphere(&mut notebook, 5.0);
    let b = add_sphere(&mut notebook, 5.0);
    let c = add_sphere(&mut notebook, 5.0);
    notebook
        .set_param(
            b,
            "center",
            ParamValue::vector(8.0, 0.0, 0.0, Unit::Millimeter),
        )
        .unwrap();
    notebook
        .set_param(
            c,
            "center",
            ParamValue::vector(-8.0, 0.0, 0.0, Unit::Millimeter),
        )
        .unwrap();

    let union = notebook.add_block("union", "Union").unwrap();
    notebook.set_overload(union, "three").unwrap();
    notebook.set_input(union, "a", a).unwrap();
    notebook.set_input(union, "b", b).unwrap();
    notebook.set_input(union, "c", c).unwrap();

    let value = notebook.evaluate(union).unwrap();
    let handle = value.as_field().unwrap();
    assert!(handle.bounds.max_extent() > 20.0);
    assert_eq!(notebook.instance(union).unwrap().status, BlockStatus::Built);

    notebook
        .set_param(a, "radius", ParamValue::number(10.0, Unit::Millimeter))
        .unwrap();
    assert!(notebook.instance(union).unwrap().dirty);
}

#[test]
fn transform_converts_angle_units_and_moves_bounds() {
    let mut notebook = Notebook::new();
    let sphere = add_sphere(&mut notebook, 5.0);
    let transform = notebook.add_block("transform", "Transform").unwrap();
    notebook.set_input(transform, "field", sphere).unwrap();
    notebook
        .set_param(
            transform,
            "translation",
            ParamValue::vector(20.0, 0.0, 0.0, Unit::Millimeter),
        )
        .unwrap();
    notebook
        .set_param(transform, "angle", ParamValue::number(90.0, Unit::Degree))
        .unwrap();

    let value = notebook.evaluate(transform).unwrap();
    let handle = value.as_field().unwrap();
    assert!((handle.bounds.center().x - 20.0).abs() < 1e-3);
    assert_eq!(
        notebook.instance(transform).unwrap().status,
        BlockStatus::Built
    );
}

#[test]
fn unchecked_nodes_can_be_evaluated_individually() {
    let mut notebook = Notebook::new();
    let sphere = add_sphere(&mut notebook, 10.0);
    let value = notebook.evaluate(sphere).unwrap();
    let handle = match value {
        Value::Field(handle) => handle,
        _ => panic!("expected field"),
    };

    let center_value = notebook.graph().evaluate_at(handle.node, Vec3::ZERO);
    assert!((center_value + 10.0).abs() < 1e-3);
    assert!(notebook.graph().bounds_of(handle.node).max_extent() > 19.9);
}

#[test]
fn list_eight_overload_unions_five_fields() {
    let mut notebook = Notebook::new();
    let mut fields = Vec::new();
    for index in 0..5 {
        let sphere = add_sphere(&mut notebook, 5.0);
        notebook
            .set_param(
                sphere,
                "center",
                ParamValue::vector(index as f32 * 10.0, 0.0, 0.0, Unit::Millimeter),
            )
            .unwrap();
        fields.push(sphere);
    }
    let list = notebook.add_block("list", "List").unwrap();
    notebook.set_overload(list, "eight").unwrap();
    for (name, field) in ["a", "b", "c", "d", "e"].iter().zip(fields) {
        notebook.set_input(list, name, field).unwrap();
    }
    let union = notebook.add_block("union", "Union").unwrap();
    notebook.set_overload(union, "list").unwrap();
    notebook.set_input(union, "items", list).unwrap();
    let value = notebook.evaluate(union).unwrap();
    let handle = value.as_field().unwrap();
    assert!(handle.bounds.max_extent() > 49.0);
}

#[test]
fn list_accepts_free_form_port_names_and_survives_snapshot() {
    let mut notebook = Notebook::new();
    let mut fields = Vec::new();
    for index in 0..3 {
        let sphere = add_sphere(&mut notebook, 5.0);
        notebook
            .set_param(
                sphere,
                "center",
                ParamValue::vector(index as f32 * 20.0, 0.0, 0.0, Unit::Millimeter),
            )
            .unwrap();
        fields.push(sphere);
    }

    let list = notebook.add_block("list", "List").unwrap();
    for (name, field) in ["p0", "p1", "p2"].iter().zip(fields) {
        notebook.set_input(list, name, field).unwrap();
    }

    let union = notebook.add_block("union", "Union").unwrap();
    notebook.set_overload(union, "list").unwrap();
    notebook.set_input(union, "items", list).unwrap();
    let value = notebook.evaluate(union).unwrap();
    let handle = value.as_field().unwrap();
    assert!(handle.bounds.max_extent() > 44.0);

    let json = notebook.to_json().unwrap();
    assert!(json.contains("\"p0\""));
    let mut restored = Notebook::from_json(&json).unwrap();
    let value = restored.evaluate(union).unwrap();
    let handle = value.as_field().unwrap();
    assert!(handle.bounds.max_extent() > 44.0);
}

#[test]
fn custom_field_expression_compiles() {
    let mut notebook = Notebook::new();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text("union(sphere(0,0,0,10), box(0,0,0,5,5,5,1))".to_string()),
        )
        .unwrap();
    let value = notebook.evaluate(custom).unwrap();
    let handle = value.as_field().unwrap();
    assert!(handle.bounds.max_extent() > 19.0);
}

#[test]
fn custom_field_reads_bound_field_inputs() {
    let mut notebook = Notebook::new();
    let a = add_sphere(&mut notebook, 5.0);
    notebook
        .set_param(
            a,
            "center",
            ParamValue::vector(-8.0, 0.0, 0.0, Unit::Millimeter),
        )
        .unwrap();
    let b = add_sphere(&mut notebook, 5.0);
    notebook
        .set_param(
            b,
            "center",
            ParamValue::vector(8.0, 0.0, 0.0, Unit::Millimeter),
        )
        .unwrap();

    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook.set_input(custom, "a", a).unwrap();
    notebook.set_input(custom, "b", b).unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text("union($a, $b)".to_string()),
        )
        .unwrap();

    let handle = notebook.evaluate(custom).unwrap().as_field().unwrap();
    assert!(handle.bounds.max_extent() > 25.0);
    assert!((handle.bounds.min.x + 13.0).abs() < 1e-3);
    assert!((handle.bounds.max.x - 13.0).abs() < 1e-3);
}

#[test]
fn custom_field_accepts_dynamic_field_ports_and_survives_snapshot() {
    let mut notebook = Notebook::new();
    let field = add_sphere(&mut notebook, 5.0);
    notebook
        .set_param(
            field,
            "center",
            ParamValue::vector(20.0, 0.0, 0.0, Unit::Millimeter),
        )
        .unwrap();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook.set_input(custom, "my_field", field).unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text("offset($my_field, 2)".to_string()),
        )
        .unwrap();

    let handle = notebook.evaluate(custom).unwrap().as_field().unwrap();
    assert!((handle.bounds.min.x - 13.0).abs() < 1e-3);
    assert!((handle.bounds.max.x - 27.0).abs() < 1e-3);

    let snapshot = notebook.to_json().unwrap();
    assert!(snapshot.contains("\"my_field\""));
    let mut reloaded = Notebook::from_json(&snapshot).unwrap();
    let handle = reloaded.evaluate(custom).unwrap().as_field().unwrap();
    assert!((handle.bounds.min.x - 13.0).abs() < 1e-3);
    assert!((handle.bounds.max.x - 27.0).abs() < 1e-3);
}

#[test]
fn custom_field_reads_bound_number_inputs() {
    let mut notebook = Notebook::new();
    let scale = notebook.add_block("number", "Scale").unwrap();
    notebook
        .set_param(scale, "value", ParamValue::number(0.5, Unit::Unitless))
        .unwrap();
    let offset = notebook.add_block("number", "Offset").unwrap();
    notebook
        .set_param(offset, "value", ParamValue::number(2.0, Unit::Millimeter))
        .unwrap();

    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook.set_input(custom, "scale", scale).unwrap();
    notebook.set_input(custom, "offset", offset).unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text("offset(sphere(0,0,0,mul(10,$scale)), $offset)".to_string()),
        )
        .unwrap();

    let handle = notebook.evaluate(custom).unwrap().as_field().unwrap();
    assert!((handle.bounds.min.x + 7.0).abs() < 1e-3);
    assert!((handle.bounds.max.x - 7.0).abs() < 1e-3);

    let snapshot = notebook.to_json().unwrap();
    assert!(snapshot.contains("\"scale\""));
    assert!(snapshot.contains("\"offset\""));
    let mut reloaded = Notebook::from_json(&snapshot).unwrap();
    let handle = reloaded.evaluate(custom).unwrap().as_field().unwrap();
    assert!((handle.bounds.min.x + 7.0).abs() < 1e-3);
    assert!((handle.bounds.max.x - 7.0).abs() < 1e-3);
}

#[test]
fn custom_field_reads_bound_vector_and_boolean_params() {
    let mut notebook = Notebook::new();
    let translation = notebook.add_block("vector", "Translation").unwrap();
    notebook
        .set_param(
            translation,
            "value",
            ParamValue::vector(20.0, 0.0, 0.0, Unit::Millimeter),
        )
        .unwrap();

    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_input(custom, "translation", translation)
        .unwrap();
    notebook
        .set_param(custom, "flag", ParamValue::Boolean(true))
        .unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text("if($flag, sphere($translation, 10), box(0,0,0,4,4,4,1))".to_string()),
        )
        .unwrap();

    let handle = notebook.evaluate(custom).unwrap().as_field().unwrap();
    assert!((handle.bounds.min.x - 10.0).abs() < 1e-3);
    assert!((handle.bounds.max.x - 30.0).abs() < 1e-3);

    notebook
        .set_param(custom, "flag", ParamValue::Boolean(false))
        .unwrap();
    let handle = notebook.evaluate(custom).unwrap().as_field().unwrap();
    assert!(handle.bounds.max_extent() < 12.0);

    let snapshot = notebook.to_json().unwrap();
    assert!(snapshot.contains("\"translation\""));
    assert!(snapshot.contains("\"flag\""));
    let mut reloaded = Notebook::from_json(&snapshot).unwrap();
    let handle = reloaded.evaluate(custom).unwrap().as_field().unwrap();
    assert!(handle.bounds.max_extent() < 12.0);
}

#[test]
fn custom_field_error_reports_line_column_and_source_snippet() {
    let mut notebook = Notebook::new();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text("let a = 1;\n".to_string()),
        )
        .unwrap();
    let error = notebook.evaluate(custom).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("at line 2, column 1"), "{message}");
    assert!(message.contains("expected value"), "{message}");
    assert!(message.contains("\n2 | "), "{message}");
    assert!(
        message
            .lines()
            .last()
            .is_some_and(|line| line.ends_with('^'))
    );
    let NotebookError::CustomField { diagnostic, origin } = error else {
        panic!("expected CustomField, got {error:?}");
    };
    assert_eq!(origin, custom);
    assert_eq!(diagnostic.line, 2);
    assert_eq!(diagnostic.column, 1);
    assert!(diagnostic.snippet.starts_with("2 | "));

    let instance = notebook.instance(custom).unwrap();
    let diagnostic = &instance.diagnostics[0];
    assert_eq!(diagnostic.source, Some(custom));
    assert_eq!(diagnostic.line, Some(2));
    assert_eq!(diagnostic.column, Some(1));
    assert!(
        diagnostic
            .snippet
            .as_deref()
            .is_some_and(|snippet| snippet.starts_with("2 | "))
    );
}

#[test]
fn custom_field_error_source_survives_mesh_failure() {
    let mut notebook = Notebook::new();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text("let a = 1;\nlet b = ;".to_string()),
        )
        .unwrap();
    let mesh = add_mesh(&mut notebook, custom, 8.0);
    let error = notebook.evaluate(mesh).unwrap_err();
    let NotebookError::CustomField { origin, .. } = error else {
        panic!("expected CustomField, got {error:?}");
    };
    assert_eq!(origin, custom);
    let diagnostic = &notebook.instance(mesh).unwrap().diagnostics[0];
    assert_eq!(diagnostic.source, Some(custom));
    assert_eq!(diagnostic.line, Some(2));
    assert_eq!(diagnostic.column, Some(9));
}

#[test]
fn custom_field_expression_supports_let_variables() {
    let mut notebook = Notebook::new();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text(
                "let a = sphere(0,0,0,10); let b = box(20,0,0,6,6,6,1); union($a,$b)".to_string(),
            ),
        )
        .unwrap();
    let value = notebook.evaluate(custom).unwrap();
    let handle = value.as_field().unwrap();
    assert!(handle.bounds.max_extent() > 25.0);
}

#[test]
fn custom_field_supports_numeric_let_variables() {
    let mut notebook = Notebook::new();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text("let r = 10; sphere(0,0,0,$r)".to_string()),
        )
        .unwrap();
    let value = notebook.evaluate(custom).unwrap();
    let handle = value.as_field().unwrap();
    assert!(handle.bounds.max_extent() > 19.0);
}

#[test]
fn custom_field_supports_vector_let_variables() {
    let mut notebook = Notebook::new();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text(
                "let c = vec(20,0,0); let h = vec(5,5,5); let a = sphere(0,0,0,10); union($a, box($c,$h,1))"
                    .to_string(),
            ),
        )
        .unwrap();
    let value = notebook.evaluate(custom).unwrap();
    let handle = value.as_field().unwrap();
    assert!(handle.bounds.max_extent() > 24.0);
}

#[test]
fn custom_field_supports_parameterized_functions() {
    let mut notebook = Notebook::new();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text(
                "def pair(r) = union(sphere(0,0,0,$r), box(20,0,0,$r,$r,$r,1)); pair(5)"
                    .to_string(),
            ),
        )
        .unwrap();
    let value = notebook.evaluate(custom).unwrap();
    let handle = value.as_field().unwrap();
    assert!(handle.bounds.max_extent() > 24.0);
}

#[test]
fn custom_field_function_body_supports_let() {
    let mut notebook = Notebook::new();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text(
                "def f(r) = { let c=vec(0,0,0); let s=sphere($c,$r); union($s, box(20,0,0,$r,$r,$r,1)); }; f(5)"
                    .to_string(),
            ),
        )
        .unwrap();
    let value = notebook.evaluate(custom).unwrap();
    let handle = value.as_field().unwrap();
    assert!(handle.bounds.max_extent() > 24.0);
}

#[test]
fn custom_field_supports_static_if() {
    let mut notebook = Notebook::new();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text(
                "def shape(r) = if(gt($r,5), sphere(0,0,0,$r), box(0,0,0,$r,$r,$r,0)); shape(10)"
                    .to_string(),
            ),
        )
        .unwrap();
    let wide = notebook.evaluate(custom).unwrap().as_field().unwrap();
    assert!(wide.bounds.max_extent() > 19.0);

    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text(
                "def shape(r) = if(gt($r,5), sphere(0,0,0,$r), box(0,0,0,$r,$r,$r,0)); shape(2)"
                    .to_string(),
            ),
        )
        .unwrap();
    let small = notebook.evaluate(custom).unwrap().as_field().unwrap();
    assert!(small.bounds.max_extent() < 5.0);
}

#[test]
fn custom_field_lazy_if_allows_recursion() {
    let mut notebook = Notebook::new();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text(
                "def f(n) = if(gt($n,0), f(sub($n,1)), sphere(0,0,0,10)); f(3)".to_string(),
            ),
        )
        .unwrap();
    let value = notebook.evaluate(custom).unwrap();
    let handle = value.as_field().unwrap();
    assert!(handle.bounds.max_extent() > 19.0);
}

#[test]
fn custom_field_for_loop_expands_fields() {
    let mut notebook = Notebook::new();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text(
                "def row(n) = for(i, 0, 3, sphere(mul($i,20),0,0,5)); row(3)".to_string(),
            ),
        )
        .unwrap();
    let value = notebook.evaluate(custom).unwrap();
    let handle = value.as_field().unwrap();
    assert!(handle.bounds.max_extent() > 49.0);
}

#[test]
fn custom_field_supports_logical_operators() {
    let mut notebook = Notebook::new();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text(
                "if(and(gt(3,2), not(lt(3,2))), sphere(0,0,0,10), box(0,0,0,2,2,2,0))".to_string(),
            ),
        )
        .unwrap();
    let value = notebook.evaluate(custom).unwrap();
    let handle = value.as_field().unwrap();
    assert!(handle.bounds.max_extent() > 19.0);
}

#[test]
fn custom_field_supports_arithmetic() {
    let mut notebook = Notebook::new();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text("def f(r) = sphere(0,0,0,mul(add($r,2),2)); f(3)".to_string()),
        )
        .unwrap();
    let value = notebook.evaluate(custom).unwrap();
    let handle = value.as_field().unwrap();
    assert!(handle.bounds.max_extent() > 19.0);
}

#[test]
fn custom_field_function_arity_is_checked() {
    let mut notebook = Notebook::new();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text(
                "def pair(r) = union(sphere(0,0,0,$r), box(20,0,0,$r,$r,$r,1)); pair()".to_string(),
            ),
        )
        .unwrap();
    assert!(matches!(
        notebook.evaluate(custom),
        Err(NotebookError::CustomField { .. })
    ));
}

#[test]
fn custom_field_unknown_variable_reports_error() {
    let mut notebook = Notebook::new();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text("union($missing, sphere(0,0,0,1))".to_string()),
        )
        .unwrap();
    assert!(matches!(
        notebook.evaluate(custom),
        Err(NotebookError::CustomField { .. })
    ));
}

#[test]
fn custom_field_invalid_expression_reports_parameter_error() {
    let mut notebook = Notebook::new();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text("nope(1)".to_string()),
        )
        .unwrap();
    assert!(matches!(
        notebook.evaluate(custom),
        Err(NotebookError::CustomField { .. })
    ));
}

#[test]
fn list_input_unions_multiple_fields() {
    let mut notebook = Notebook::new();
    let a = add_sphere(&mut notebook, 5.0);
    let b = add_sphere(&mut notebook, 5.0);
    notebook
        .set_param(
            b,
            "center",
            ParamValue::vector(20.0, 0.0, 0.0, Unit::Millimeter),
        )
        .unwrap();

    let list = notebook.add_block("list", "List").unwrap();
    notebook.set_input(list, "a", a).unwrap();
    notebook.set_input(list, "b", b).unwrap();

    let union = notebook.add_block("union", "Union").unwrap();
    notebook.set_overload(union, "list").unwrap();
    notebook.set_input(union, "items", list).unwrap();

    let value = notebook.evaluate(union).unwrap();
    let handle = value.as_field().unwrap();
    assert!(handle.bounds.max_extent() > 24.0);
}

#[test]
fn mesh_block_can_split_crease_normals() {
    let mut notebook = Notebook::new();
    let sphere = add_sphere(&mut notebook, 10.0);
    let mesh = add_mesh(&mut notebook, sphere, 32.0);
    let smooth = mesh_value(&mut notebook, mesh);
    notebook
        .set_param(mesh, "crease", ParamValue::number(30.0, Unit::Degree))
        .unwrap();
    let split = mesh_value(&mut notebook, mesh);
    assert_eq!(split.triangle_count(), smooth.triangle_count());
    assert!(split.vertex_count() > smooth.vertex_count());
}

#[test]
fn mesh_block_can_simplify_before_output() {
    let mut notebook = Notebook::new();
    let sphere = add_sphere(&mut notebook, 10.0);
    let mesh = add_mesh(&mut notebook, sphere, 32.0);
    let original = mesh_value(&mut notebook, mesh);
    notebook
        .set_param(mesh, "simplify", ParamValue::number(0.5, Unit::Unitless))
        .unwrap();
    notebook
        .set_param(
            mesh,
            "simplify_angle",
            ParamValue::number(45.0, Unit::Degree),
        )
        .unwrap();
    let simplified = mesh_value(&mut notebook, mesh);
    assert!(simplified.triangle_count() > 0);
    assert!(simplified.triangle_count() < original.triangle_count());
}

#[test]
fn mesh_block_can_relax_feature_chains() {
    let mut notebook = Notebook::new();
    let custom = notebook.add_block("custom", "Custom").unwrap();
    notebook
        .set_param(
            custom,
            "expression",
            ParamValue::Text("box(0,0,0,10,10,10,0)".to_string()),
        )
        .unwrap();
    let mesh = add_mesh(&mut notebook, custom, 32.0);
    let original = mesh_value(&mut notebook, mesh);
    notebook
        .set_param(
            mesh,
            "feature_relax",
            ParamValue::number(2.0, Unit::Unitless),
        )
        .unwrap();
    let relaxed = mesh_value(&mut notebook, mesh);
    assert_eq!(relaxed.triangle_count(), original.triangle_count());
    assert_eq!(relaxed.vertex_count(), original.vertex_count());
    assert!(relaxed.stats().is_closed);
}

#[test]
fn mesh_block_cap_is_noop_on_closed_mesh() {
    let mut notebook = Notebook::new();
    let sphere = add_sphere(&mut notebook, 10.0);
    let mesh = add_mesh(&mut notebook, sphere, 24.0);
    let closed = mesh_value(&mut notebook, mesh);
    notebook
        .set_param(mesh, "cap", ParamValue::Boolean(true))
        .unwrap();
    let capped = mesh_value(&mut notebook, mesh);
    assert_eq!(capped.triangle_count(), closed.triangle_count());
    assert!(capped.stats().is_closed);
}

#[test]
fn mesh_dual_contour_overload_is_closed() {
    let mut notebook = Notebook::new();
    let sphere = add_sphere(&mut notebook, 10.0);
    let mesh = add_mesh(&mut notebook, sphere, 32.0);
    notebook.set_overload(mesh, "dual_contour").unwrap();

    let value = notebook.evaluate(mesh).unwrap();
    let mesh = match value {
        Value::Mesh(mesh) => mesh,
        other => panic!("expected mesh, got {:?}", other.value_type()),
    };
    let stats = mesh.stats();
    assert!(stats.triangles > 0);
    assert!(stats.is_closed);
    assert_eq!(stats.boundary_edges, 0);
}
