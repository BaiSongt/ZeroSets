use std::sync::Arc;

use zeroset_blocks::{
    InstanceId, Notebook, NotebookError, ParamValue, SNAPSHOT_VERSION, Unit, UpdatePolicy, Value,
};

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
fn snapshot_roundtrip_preserves_ids_units_and_links() {
    let mut notebook = Notebook::new();
    let sphere = add_sphere(&mut notebook, 10.0);
    notebook.set_overload(sphere, "diameter").unwrap();
    notebook
        .set_param(
            sphere,
            "diameter",
            ParamValue::number(25.4, Unit::Millimeter),
        )
        .unwrap();
    let mesh = add_mesh(&mut notebook, sphere, 20.0);
    notebook.set_update_policy(UpdatePolicy::Manual);

    let original = mesh_value(&mut notebook, mesh);
    let json = notebook.to_json().unwrap();
    assert!(json.contains("\"millimeter\""));
    assert!(json.contains("\"diameter\""));

    let mut restored = Notebook::from_json(&json).unwrap();
    assert_eq!(restored.instances().count(), notebook.instances().count());
    assert_eq!(
        restored.instance(sphere).unwrap().overload.as_deref(),
        Some("diameter")
    );
    assert_eq!(
        restored.instance(mesh).unwrap().inputs.get("field"),
        Some(&sphere)
    );
    assert_eq!(restored.update_policy(), UpdatePolicy::Manual);

    let replayed = mesh_value(&mut restored, mesh);
    assert_eq!(replayed.triangle_count(), original.triangle_count());
    assert_eq!(replayed.positions.len(), original.positions.len());
}

#[test]
fn snapshot_rejects_bad_versions_and_missing_sources() {
    let mut notebook = Notebook::new();
    let _sphere = add_sphere(&mut notebook, 10.0);
    let mut snapshot = notebook.snapshot();
    snapshot.version = SNAPSHOT_VERSION + 1;
    assert!(matches!(
        Notebook::from_snapshot(snapshot),
        Err(NotebookError::UnsupportedSnapshotVersion(_))
    ));

    let mut notebook = Notebook::new();
    let _mesh = notebook.add_block("mesh", "Mesh").unwrap();
    let mut snapshot = notebook.snapshot();
    snapshot.instances[0]
        .inputs
        .insert("field".to_string(), 999);
    let error = Notebook::from_snapshot(snapshot).unwrap_err();
    assert!(matches!(error, NotebookError::InvalidSnapshot(_)));
}
