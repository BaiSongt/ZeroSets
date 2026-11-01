use glam::Vec3;

use crate::FieldGraph;

/// Unit sphere at the origin.
pub fn sphere() -> FieldGraph {
    let mut graph = FieldGraph::new();
    let root = graph.sphere(Vec3::ZERO, 1.0);
    graph.set_root(root);
    graph
}

/// Rounded box, useful as a primitive sanity check.
pub fn rounded_box() -> FieldGraph {
    let mut graph = FieldGraph::new();
    let root = graph.rounded_box(Vec3::ZERO, Vec3::new(1.0, 0.75, 0.5), 0.15);
    graph.set_root(root);
    graph
}

/// Mechanical bracket from the first volume's capstone:
/// rounded base + capsule ribs (smooth union) + four through holes (subtract).
pub fn bracket() -> FieldGraph {
    let mut graph = FieldGraph::new();

    let base = graph.rounded_box(Vec3::ZERO, Vec3::new(30.0, 20.0, 6.0), 2.0);
    let mut root = base;

    for y in [-10.0, 0.0, 10.0] {
        let rib = graph.capsule(Vec3::new(-24.0, y, 6.0), Vec3::new(24.0, y, 6.0), 2.0);
        root = graph.smooth_union(root, rib, 1.5);
    }

    for x in [-18.0, 18.0] {
        for y in [-10.0, 10.0] {
            let hole = graph.cylinder(Vec3::new(x, y, 0.0), Vec3::Z, 3.5, 10.0);
            root = graph.subtract(root, hole);
        }
    }

    graph.set_root(root);
    graph
}

/// Transform demo: rotated, translated bracket.
pub fn bracket_rotated() -> FieldGraph {
    let mut graph = bracket();
    let root = graph.root().expect("bracket has a root");
    let transform = crate::Transform::new(
        Vec3::new(0.0, 0.0, 10.0),
        glam::Quat::from_rotation_x(std::f32::consts::FRAC_PI_6),
        1.0,
    )
    .expect("valid transform");
    let transformed = graph.transform(root, transform);
    graph.set_root(transformed);
    graph
}

pub fn by_name(name: &str) -> Option<FieldGraph> {
    match name {
        "sphere" => Some(sphere()),
        "box" | "rounded_box" => Some(rounded_box()),
        "bracket" => Some(bracket()),
        "bracket_rotated" => Some(bracket_rotated()),
        _ => None,
    }
}

pub const PRESETS: &[&str] = &["sphere", "rounded_box", "bracket", "bracket_rotated"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FieldCategory;

    #[test]
    fn bracket_has_finite_bounds_and_is_approximate() {
        let graph = bracket();
        graph.validate().unwrap();
        assert!(!graph.bounds().is_empty());
        assert_eq!(graph.category(), FieldCategory::ApproximateSdf);
    }

    #[test]
    fn bracket_has_holes() {
        let graph = bracket();
        // A hole center should be outside the solid, even though it lies inside
        // the base box.
        assert!(graph.evaluate(Vec3::new(18.0, 10.0, 0.0)) > 0.0);
        // A point in the base between holes is inside.
        assert!(graph.evaluate(Vec3::new(0.0, 0.0, 0.0)) < 0.0);
    }
}
