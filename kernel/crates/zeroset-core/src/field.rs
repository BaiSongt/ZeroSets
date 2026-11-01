use std::collections::HashSet;

use glam::{DQuat, DVec2, DVec3, Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::{Aabb, KernelError};

/// Stable index into a [`FieldGraph`]'s arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub u32);

/// How much the scalar field can be trusted as a distance field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldCategory {
    /// `|grad f| == 1` almost everywhere and the value is a true distance.
    ExactSdf,
    /// Signed, but only an approximation of distance (CSG, smooth blends, ...).
    ApproximateSdf,
    /// Arbitrary scalar field; zero set still valid, distance semantics are not.
    GeneralField,
}

pub fn category_name(category: FieldCategory) -> &'static str {
    match category {
        FieldCategory::ExactSdf => "exact_sdf",
        FieldCategory::ApproximateSdf => "approximate_sdf",
        FieldCategory::GeneralField => "general_field",
    }
}

/// Rigid transform plus uniform scale, stored as the *forward* map.
///
/// Evaluation uses the inverse map, which is why the graph stores one transform
/// per node instead of baking matrices into every primitive.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: f32,
}

impl Transform {
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: 1.0,
    };

    pub fn new(translation: Vec3, rotation: Quat, scale: f32) -> Result<Self, KernelError> {
        if !translation.is_finite() || !rotation.is_finite() {
            return Err(KernelError::InvalidTransform);
        }
        if !scale.is_finite() || scale <= 0.0 {
            return Err(KernelError::InvalidScale(scale));
        }
        if rotation.length_squared() < 1e-12 {
            return Err(KernelError::InvalidTransform);
        }
        Ok(Self {
            translation,
            rotation: rotation.normalize(),
            scale,
        })
    }

    pub fn from_translation(translation: Vec3) -> Self {
        Self {
            translation,
            ..Self::IDENTITY
        }
    }

    pub fn from_rotation(rotation: Quat) -> Result<Self, KernelError> {
        Self::new(Vec3::ZERO, rotation, 1.0)
    }

    pub fn from_scale(scale: f32) -> Result<Self, KernelError> {
        Self::new(Vec3::ZERO, Quat::IDENTITY, scale)
    }

    #[inline]
    pub fn apply_point(&self, point: Vec3) -> Vec3 {
        self.rotation * (point * self.scale) + self.translation
    }

    #[inline]
    pub fn inverse_point(&self, point: Vec3) -> Vec3 {
        self.rotation.inverse() * (point - self.translation) / self.scale
    }

    #[inline]
    pub fn inverse_direction(&self, direction: Vec3) -> Vec3 {
        self.rotation.inverse() * direction
    }

    /// Conservative world-space AABB of a local AABB.
    pub fn transform_aabb(&self, bounds: Aabb) -> Aabb {
        if bounds.is_empty() {
            return bounds;
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for corner in bounds.corners() {
            let point = self.apply_point(corner);
            min = min.min(point);
            max = max.max(point);
        }
        Aabb { min, max }
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// One node of the field DAG.
///
/// The JSON representation is tagged by `op`, e.g.
/// `{"op":"sphere","center":[0,0,0],"radius":1.0}`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Node {
    Sphere {
        center: Vec3,
        radius: f32,
    },
    Box {
        center: Vec3,
        half: Vec3,
        #[serde(default)]
        rounding: f32,
    },
    Cylinder {
        center: Vec3,
        axis: Vec3,
        radius: f32,
        half_height: f32,
    },
    Capsule {
        a: Vec3,
        b: Vec3,
        radius: f32,
    },
    Torus {
        center: Vec3,
        normal: Vec3,
        major: f32,
        minor: f32,
    },
    Union {
        a: NodeId,
        b: NodeId,
    },
    Intersect {
        a: NodeId,
        b: NodeId,
    },
    Subtract {
        a: NodeId,
        b: NodeId,
    },
    SmoothUnion {
        a: NodeId,
        b: NodeId,
        k: f32,
    },
    SmoothIntersect {
        a: NodeId,
        b: NodeId,
        k: f32,
    },
    SmoothSubtract {
        a: NodeId,
        b: NodeId,
        k: f32,
    },
    Offset {
        child: NodeId,
        distance: f32,
    },
    Transform {
        child: NodeId,
        transform: Transform,
    },
}

impl Node {
    pub fn children(&self) -> [Option<NodeId>; 2] {
        match self {
            Self::Union { a, b }
            | Self::Intersect { a, b }
            | Self::Subtract { a, b }
            | Self::SmoothUnion { a, b, .. }
            | Self::SmoothIntersect { a, b, .. }
            | Self::SmoothSubtract { a, b, .. } => [Some(*a), Some(*b)],
            Self::Offset { child, .. } | Self::Transform { child, .. } => [Some(*child), None],
            _ => [None, None],
        }
    }

    pub fn op_name(&self) -> &'static str {
        match self {
            Self::Sphere { .. } => "sphere",
            Self::Box { .. } => "box",
            Self::Cylinder { .. } => "cylinder",
            Self::Capsule { .. } => "capsule",
            Self::Torus { .. } => "torus",
            Self::Union { .. } => "union",
            Self::Intersect { .. } => "intersect",
            Self::Subtract { .. } => "subtract",
            Self::SmoothUnion { .. } => "smooth_union",
            Self::SmoothIntersect { .. } => "smooth_intersect",
            Self::SmoothSubtract { .. } => "smooth_subtract",
            Self::Offset { .. } => "offset",
            Self::Transform { .. } => "transform",
        }
    }
}

/// Immutable-by-convention DAG of scalar field nodes.
///
/// Nodes are appended; the graph is evaluated from `root`. Sharing is explicit:
/// two parents can reference the same `NodeId`, which is what makes the DAG a
/// DAG instead of a tree.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FieldGraph {
    nodes: Vec<Node>,
    root: Option<NodeId>,
}

impl FieldGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(capacity),
            root: None,
        }
    }

    pub fn add(&mut self, node: Node) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(node);
        id
    }

    pub fn root(&self) -> Option<NodeId> {
        self.root
    }

    pub fn set_root(&mut self, root: NodeId) {
        self.root = Some(root);
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.0 as usize)
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Builds a copy containing only the nodes reachable from `roots`.
    ///
    /// Orphaned nodes (for example, superseded field subgraphs) are dropped and
    /// surviving nodes are renumbered in dependency order. Shared children are
    /// copied once. The returned vector maps each old node id to its new id;
    /// dropped nodes map to `None`.
    pub fn compacted(&self, roots: &[NodeId]) -> (Self, Vec<Option<NodeId>>) {
        let mut graph = Self::with_capacity(self.nodes.len());
        let mut remap = vec![None; self.nodes.len()];
        let mut state = vec![0u8; self.nodes.len()];
        for root in roots {
            self.copy_reachable(*root, &mut graph, &mut remap, &mut state);
        }
        graph.root = self
            .root
            .and_then(|root| remap.get(root.0 as usize).copied().flatten());
        (graph, remap)
    }

    fn copy_reachable(
        &self,
        id: NodeId,
        graph: &mut Self,
        remap: &mut [Option<NodeId>],
        state: &mut [u8],
    ) -> Option<NodeId> {
        let index = id.0 as usize;
        let source = self.nodes.get(index)?.clone();
        if let Some(new_id) = remap[index] {
            return Some(new_id);
        }
        // A cycle in a hand-built graph would otherwise recurse forever.
        if state[index] == 1 {
            return None;
        }
        state[index] = 1;
        let node = match source {
            Node::Union { a, b } => Node::Union {
                a: self.copy_reachable(a, graph, remap, state)?,
                b: self.copy_reachable(b, graph, remap, state)?,
            },
            Node::Intersect { a, b } => Node::Intersect {
                a: self.copy_reachable(a, graph, remap, state)?,
                b: self.copy_reachable(b, graph, remap, state)?,
            },
            Node::Subtract { a, b } => Node::Subtract {
                a: self.copy_reachable(a, graph, remap, state)?,
                b: self.copy_reachable(b, graph, remap, state)?,
            },
            Node::SmoothUnion { a, b, k } => Node::SmoothUnion {
                a: self.copy_reachable(a, graph, remap, state)?,
                b: self.copy_reachable(b, graph, remap, state)?,
                k,
            },
            Node::SmoothIntersect { a, b, k } => Node::SmoothIntersect {
                a: self.copy_reachable(a, graph, remap, state)?,
                b: self.copy_reachable(b, graph, remap, state)?,
                k,
            },
            Node::SmoothSubtract { a, b, k } => Node::SmoothSubtract {
                a: self.copy_reachable(a, graph, remap, state)?,
                b: self.copy_reachable(b, graph, remap, state)?,
                k,
            },
            Node::Offset { child, distance } => Node::Offset {
                child: self.copy_reachable(child, graph, remap, state)?,
                distance,
            },
            Node::Transform { child, transform } => Node::Transform {
                child: self.copy_reachable(child, graph, remap, state)?,
                transform,
            },
            primitive => primitive,
        };
        let new_id = graph.add(node);
        remap[index] = Some(new_id);
        state[index] = 2;
        Some(new_id)
    }

    // ---------------------------------------------------------------------
    // Builder helpers
    // ---------------------------------------------------------------------

    pub fn sphere(&mut self, center: Vec3, radius: f32) -> NodeId {
        self.add(Node::Sphere { center, radius })
    }

    pub fn rounded_box(&mut self, center: Vec3, half: Vec3, rounding: f32) -> NodeId {
        self.add(Node::Box {
            center,
            half,
            rounding,
        })
    }

    pub fn cylinder(&mut self, center: Vec3, axis: Vec3, radius: f32, half_height: f32) -> NodeId {
        self.add(Node::Cylinder {
            center,
            axis,
            radius,
            half_height,
        })
    }

    pub fn capsule(&mut self, a: Vec3, b: Vec3, radius: f32) -> NodeId {
        self.add(Node::Capsule { a, b, radius })
    }

    pub fn torus(&mut self, center: Vec3, normal: Vec3, major: f32, minor: f32) -> NodeId {
        self.add(Node::Torus {
            center,
            normal,
            major,
            minor,
        })
    }

    pub fn union(&mut self, a: NodeId, b: NodeId) -> NodeId {
        self.add(Node::Union { a, b })
    }

    pub fn intersect(&mut self, a: NodeId, b: NodeId) -> NodeId {
        self.add(Node::Intersect { a, b })
    }

    pub fn subtract(&mut self, a: NodeId, b: NodeId) -> NodeId {
        self.add(Node::Subtract { a, b })
    }

    pub fn smooth_union(&mut self, a: NodeId, b: NodeId, k: f32) -> NodeId {
        self.add(Node::SmoothUnion { a, b, k })
    }

    pub fn smooth_intersect(&mut self, a: NodeId, b: NodeId, k: f32) -> NodeId {
        self.add(Node::SmoothIntersect { a, b, k })
    }

    pub fn smooth_subtract(&mut self, a: NodeId, b: NodeId, k: f32) -> NodeId {
        self.add(Node::SmoothSubtract { a, b, k })
    }

    pub fn offset(&mut self, child: NodeId, distance: f32) -> NodeId {
        self.add(Node::Offset { child, distance })
    }

    pub fn transform(&mut self, child: NodeId, transform: Transform) -> NodeId {
        self.add(Node::Transform { child, transform })
    }

    /// Return a copy evaluated in a local frame: `g'(p) == g(p + reference)`.
    ///
    /// Positions are shifted only in the frame they live in: root-frame
    /// primitives and root-frame transform translations move by `-reference`;
    /// primitives inside a transform are already in that transform's local
    /// frame and stay untouched. This is the kernel-side precision trick for
    /// models far from the world origin: evaluate small local coordinates on
    /// the GPU, then add the reference back when emitting world-space vertices.
    ///
    /// Shared nodes referenced from two different frames are rebased on their
    /// first encounter; keep shared subgraphs inside one frame.
    pub fn rebased(&self, reference: Vec3) -> Self {
        let mut graph = self.clone();
        if let Some(root) = self.root {
            let mut visited = HashSet::new();
            rebase_node(&mut graph.nodes, root, -reference, true, &mut visited);
        }
        graph
    }

    // ---------------------------------------------------------------------
    // Validation
    // ---------------------------------------------------------------------

    pub fn validate(&self) -> Result<(), KernelError> {
        let root = self.root.ok_or(KernelError::EmptyGraph)?;
        if root.0 as usize >= self.nodes.len() {
            return Err(KernelError::InvalidNode(root.0));
        }
        for node in &self.nodes {
            for child in node.children().into_iter().flatten() {
                if child.0 as usize >= self.nodes.len() {
                    return Err(KernelError::InvalidNode(child.0));
                }
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Evaluation
    // ---------------------------------------------------------------------

    /// Evaluate the field at `point`.
    ///
    /// An empty graph returns `+inf`, which is the "outside everywhere" field.
    #[inline]
    pub fn evaluate(&self, point: Vec3) -> f32 {
        match self.root {
            Some(root) => self.eval_node(root, point),
            None => f32::INFINITY,
        }
    }

    /// Evaluate a specific node, including nodes that are not the graph root.
    /// This is what the block layer uses to compose fields without cloning the
    /// whole DAG.
    #[inline]
    pub fn evaluate_at(&self, node: NodeId, point: Vec3) -> f32 {
        self.eval_node(node, point)
    }

    /// f64 reference evaluation of the graph root.
    ///
    /// Node parameters are still stored as f32; this path converts them to f64
    /// and performs every distance/CSG operation in double precision. It is a
    /// golden/reference backend, not a replacement for the f32 GPU path.
    pub fn evaluate_f64(&self, point: DVec3) -> f64 {
        match self.root {
            Some(root) => self.eval_node_f64(root, point),
            None => f64::INFINITY,
        }
    }

    /// f64 reference evaluation of a specific node.
    pub fn evaluate_at_f64(&self, node: NodeId, point: DVec3) -> f64 {
        self.eval_node_f64(node, point)
    }

    /// Central-difference gradient in f64, used as a precision reference.
    pub fn gradient_f64(&self, point: DVec3) -> DVec3 {
        if self.root.is_none() {
            return DVec3::splat(f64::INFINITY);
        }
        if let Some(gradient) = self.gradient_f64_analytic(point) {
            if gradient.is_finite() {
                return gradient;
            }
        }
        let h = 1e-9 * (1.0 + point.abs().max_element());
        let dx = DVec3::X * h;
        let dy = DVec3::Y * h;
        let dz = DVec3::Z * h;
        let denominator = 2.0 * h;
        DVec3::new(
            (self.evaluate_f64(point + dx) - self.evaluate_f64(point - dx)) / denominator,
            (self.evaluate_f64(point + dy) - self.evaluate_f64(point - dy)) / denominator,
            (self.evaluate_f64(point + dz) - self.evaluate_f64(point - dz)) / denominator,
        )
    }

    fn eval_node_f64(&self, id: NodeId, point: DVec3) -> f64 {
        match &self.nodes[id.0 as usize] {
            Node::Sphere { center, radius } => point.distance(dvec3(*center)) - *radius as f64,
            Node::Box {
                center,
                half,
                rounding,
            } => sd_round_box_f64(point, dvec3(*center), dvec3(*half), *rounding as f64),
            Node::Cylinder {
                center,
                axis,
                radius,
                half_height,
            } => sd_cylinder_f64(
                point,
                dvec3(*center),
                dvec3(*axis),
                *radius as f64,
                *half_height as f64,
            ),
            Node::Capsule { a, b, radius } => {
                sd_capsule_f64(point, dvec3(*a), dvec3(*b), *radius as f64)
            }
            Node::Torus {
                center,
                normal,
                major,
                minor,
            } => sd_torus_f64(
                point,
                dvec3(*center),
                dvec3(*normal),
                *major as f64,
                *minor as f64,
            ),
            Node::Union { a, b } => self
                .eval_node_f64(*a, point)
                .min(self.eval_node_f64(*b, point)),
            Node::Intersect { a, b } => self
                .eval_node_f64(*a, point)
                .max(self.eval_node_f64(*b, point)),
            Node::Subtract { a, b } => self
                .eval_node_f64(*a, point)
                .max(-self.eval_node_f64(*b, point)),
            Node::SmoothUnion { a, b, k } => smin_f64(
                self.eval_node_f64(*a, point),
                self.eval_node_f64(*b, point),
                *k as f64,
            ),
            Node::SmoothIntersect { a, b, k } => smax_f64(
                self.eval_node_f64(*a, point),
                self.eval_node_f64(*b, point),
                *k as f64,
            ),
            Node::SmoothSubtract { a, b, k } => smax_f64(
                self.eval_node_f64(*a, point),
                -self.eval_node_f64(*b, point),
                *k as f64,
            ),
            Node::Offset { child, distance } => {
                self.eval_node_f64(*child, point) - *distance as f64
            }
            Node::Transform { child, transform } => {
                let local = inverse_point_f64(*transform, point);
                self.eval_node_f64(*child, local) * transform.scale as f64
            }
        }
    }

    /// Analytic f64 gradient where a node provides one.
    pub fn gradient_f64_analytic(&self, point: DVec3) -> Option<DVec3> {
        self.root.and_then(|root| self.grad_node_f64(root, point))
    }

    fn grad_node_f64(&self, id: NodeId, point: DVec3) -> Option<DVec3> {
        match &self.nodes[id.0 as usize] {
            Node::Sphere { center, .. } => normalize_or_none_f64(point - dvec3(*center)),
            Node::Box {
                center,
                half,
                rounding,
            } => grad_round_box_f64(point, dvec3(*center), dvec3(*half), *rounding as f64),
            Node::Cylinder {
                center,
                axis,
                radius,
                half_height,
            } => grad_cylinder_f64(
                point,
                dvec3(*center),
                dvec3(*axis),
                *radius as f64,
                *half_height as f64,
            ),
            Node::Capsule { a, b, .. } => {
                let a = dvec3(*a);
                let b = dvec3(*b);
                let pa = point - a;
                let ba = b - a;
                let denominator = ba.length_squared();
                let h = if denominator > 1e-24 {
                    (pa.dot(ba) / denominator).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                normalize_or_none_f64(point - (a + ba * h))
            }
            Node::Torus {
                center,
                normal,
                major,
                minor,
            } => grad_torus_f64(
                point,
                dvec3(*center),
                dvec3(*normal),
                *major as f64,
                *minor as f64,
            ),
            Node::Union { a, b } => {
                let va = self.eval_node_f64(*a, point);
                let vb = self.eval_node_f64(*b, point);
                if (va - vb).abs() <= f64::EPSILON {
                    None
                } else if va < vb {
                    self.grad_node_f64(*a, point)
                } else {
                    self.grad_node_f64(*b, point)
                }
            }
            Node::Intersect { a, b } => {
                let va = self.eval_node_f64(*a, point);
                let vb = self.eval_node_f64(*b, point);
                if (va - vb).abs() <= f64::EPSILON {
                    None
                } else if va > vb {
                    self.grad_node_f64(*a, point)
                } else {
                    self.grad_node_f64(*b, point)
                }
            }
            Node::Subtract { a, b } => {
                let va = self.eval_node_f64(*a, point);
                let vb = -self.eval_node_f64(*b, point);
                if (va - vb).abs() <= f64::EPSILON {
                    None
                } else if va > vb {
                    self.grad_node_f64(*a, point)
                } else {
                    self.grad_node_f64(*b, point).map(|gradient| -gradient)
                }
            }
            Node::SmoothUnion { a, b, k } => {
                let va = self.eval_node_f64(*a, point);
                let vb = self.eval_node_f64(*b, point);
                let h = smooth_blend_f64(va, vb, *k as f64, false);
                let ga = self.grad_node_f64(*a, point)?;
                let gb = self.grad_node_f64(*b, point)?;
                Some(ga * h + gb * (1.0 - h))
            }
            Node::SmoothIntersect { a, b, k } => {
                let va = self.eval_node_f64(*a, point);
                let vb = self.eval_node_f64(*b, point);
                let h = smooth_blend_f64(va, vb, *k as f64, true);
                let ga = self.grad_node_f64(*a, point)?;
                let gb = self.grad_node_f64(*b, point)?;
                Some(ga * h + gb * (1.0 - h))
            }
            Node::SmoothSubtract { a, b, k } => {
                let va = self.eval_node_f64(*a, point);
                let vb = self.eval_node_f64(*b, point);
                let h = smooth_blend_f64(va, -vb, *k as f64, true);
                let ga = self.grad_node_f64(*a, point)?;
                let gb = self.grad_node_f64(*b, point)?;
                Some(ga * h - gb * (1.0 - h))
            }
            Node::Offset { child, .. } => self.grad_node_f64(*child, point),
            Node::Transform { child, transform } => {
                let local = inverse_point_f64(*transform, point);
                let gradient = self.grad_node_f64(*child, local)?;
                Some(rotation_f64(*transform) * gradient)
            }
        }
    }

    /// Bounds of a specific node.
    pub fn bounds_of(&self, node: NodeId) -> Aabb {
        self.node_bounds(node)
    }

    /// Category of a specific node.
    pub fn category_of(&self, node: NodeId) -> FieldCategory {
        self.node_category(node)
    }

    fn eval_node(&self, id: NodeId, point: Vec3) -> f32 {
        match &self.nodes[id.0 as usize] {
            Node::Sphere { center, radius } => point.distance(*center) - radius,
            Node::Box {
                center,
                half,
                rounding,
            } => sd_round_box(point, *center, *half, *rounding),
            Node::Cylinder {
                center,
                axis,
                radius,
                half_height,
            } => sd_cylinder(point, *center, *axis, *radius, *half_height),
            Node::Capsule { a, b, radius } => sd_capsule(point, *a, *b, *radius),
            Node::Torus {
                center,
                normal,
                major,
                minor,
            } => sd_torus(point, *center, *normal, *major, *minor),
            Node::Union { a, b } => self.eval_node(*a, point).min(self.eval_node(*b, point)),
            Node::Intersect { a, b } => self.eval_node(*a, point).max(self.eval_node(*b, point)),
            Node::Subtract { a, b } => self.eval_node(*a, point).max(-self.eval_node(*b, point)),
            Node::SmoothUnion { a, b, k } => {
                smin(self.eval_node(*a, point), self.eval_node(*b, point), *k)
            }
            Node::SmoothIntersect { a, b, k } => {
                smax(self.eval_node(*a, point), self.eval_node(*b, point), *k)
            }
            Node::SmoothSubtract { a, b, k } => {
                smax(self.eval_node(*a, point), -self.eval_node(*b, point), *k)
            }
            Node::Offset { child, distance } => self.eval_node(*child, point) - distance,
            Node::Transform { child, transform } => {
                self.eval_node(*child, transform.inverse_point(point)) * transform.scale
            }
        }
    }

    /// Gradient at `point`.
    ///
    /// Analytic where a node provides one; numerical central differences at
    /// singularities or for nodes without a closed-form gradient.
    pub fn gradient(&self, point: Vec3) -> Vec3 {
        if let Some(gradient) = self.gradient_analytic(point) {
            if gradient.is_finite() {
                return gradient;
            }
        }
        self.gradient_numerical(point)
    }

    /// Analytic gradient of the field, if every node on the active branch has a
    /// well-defined closed-form gradient at this point.
    pub fn gradient_analytic(&self, point: Vec3) -> Option<Vec3> {
        self.root.and_then(|root| self.grad_node(root, point))
    }

    fn grad_node(&self, id: NodeId, point: Vec3) -> Option<Vec3> {
        match &self.nodes[id.0 as usize] {
            Node::Sphere { center, .. } => normalize_or_none(point - *center),
            Node::Box {
                center,
                half,
                rounding,
            } => grad_round_box(point, *center, *half, *rounding),
            Node::Cylinder {
                center,
                axis,
                radius,
                half_height,
            } => grad_cylinder(point, *center, *axis, *radius, *half_height),
            Node::Capsule { a, b, .. } => {
                let pa = point - *a;
                let ba = *b - *a;
                let denom = ba.length_squared();
                let h = if denom > 1e-12 {
                    (pa.dot(ba) / denom).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                normalize_or_none(point - (*a + ba * h))
            }
            Node::Torus {
                center,
                normal,
                major,
                minor,
            } => grad_torus(point, *center, *normal, *major, *minor),
            Node::Union { a, b } => {
                let va = self.eval_node(*a, point);
                let vb = self.eval_node(*b, point);
                if (va - vb).abs() <= f32::EPSILON {
                    None
                } else if va < vb {
                    self.grad_node(*a, point)
                } else {
                    self.grad_node(*b, point)
                }
            }
            Node::Intersect { a, b } => {
                let va = self.eval_node(*a, point);
                let vb = self.eval_node(*b, point);
                if (va - vb).abs() <= f32::EPSILON {
                    None
                } else if va > vb {
                    self.grad_node(*a, point)
                } else {
                    self.grad_node(*b, point)
                }
            }
            Node::Subtract { a, b } => {
                let va = self.eval_node(*a, point);
                let vb = -self.eval_node(*b, point);
                if (va - vb).abs() <= f32::EPSILON {
                    None
                } else if va > vb {
                    self.grad_node(*a, point)
                } else {
                    self.grad_node(*b, point).map(|gradient| -gradient)
                }
            }
            Node::SmoothUnion { a, b, k } => {
                let va = self.eval_node(*a, point);
                let vb = self.eval_node(*b, point);
                let h = smooth_blend(va, vb, *k, false);
                let ga = self.grad_node(*a, point)?;
                let gb = self.grad_node(*b, point)?;
                Some(ga * h + gb * (1.0 - h))
            }
            Node::SmoothIntersect { a, b, k } => {
                let va = self.eval_node(*a, point);
                let vb = self.eval_node(*b, point);
                let h = smooth_blend(va, vb, *k, true);
                let ga = self.grad_node(*a, point)?;
                let gb = self.grad_node(*b, point)?;
                Some(ga * h + gb * (1.0 - h))
            }
            Node::SmoothSubtract { a, b, k } => {
                let va = self.eval_node(*a, point);
                let vb = self.eval_node(*b, point);
                let h = smooth_blend(va, -vb, *k, true);
                let ga = self.grad_node(*a, point)?;
                let gb = self.grad_node(*b, point)?;
                Some(ga * h - gb * (1.0 - h))
            }
            Node::Offset { child, .. } => self.grad_node(*child, point),
            Node::Transform { child, transform } => {
                let local = transform.inverse_point(point);
                let gradient = self.grad_node(*child, local)?;
                Some(transform.rotation * gradient)
            }
        }
    }

    /// Central-difference gradient, used as a fallback and for comparisons.
    pub fn gradient_numerical(&self, point: Vec3) -> Vec3 {
        let h = self.gradient_step(point);
        let dx = Vec3::X * h;
        let dy = Vec3::Y * h;
        let dz = Vec3::Z * h;
        let denom = 2.0 * h;
        Vec3::new(
            (self.evaluate(point + dx) - self.evaluate(point - dx)) / denom,
            (self.evaluate(point + dy) - self.evaluate(point - dy)) / denom,
            (self.evaluate(point + dz) - self.evaluate(point - dz)) / denom,
        )
    }

    pub fn gradient_step(&self, point: Vec3) -> f32 {
        let bounds = self.bounds();
        let model_scale = if bounds.is_empty() {
            1.0
        } else {
            bounds.max_extent().max(1.0)
        };
        let local_scale = point.length().max(1.0);
        (model_scale.max(local_scale) * 1e-3).clamp(1e-4, 1.0)
    }

    // ---------------------------------------------------------------------
    // Bounds and classification
    // ---------------------------------------------------------------------

    pub fn bounds(&self) -> Aabb {
        match self.root {
            Some(root) => self.node_bounds(root),
            None => Aabb::EMPTY,
        }
    }

    fn node_bounds(&self, id: NodeId) -> Aabb {
        match &self.nodes[id.0 as usize] {
            Node::Sphere { center, radius } => {
                Aabb::from_center_half(*center, Vec3::splat(radius.abs()))
            }
            Node::Box {
                center,
                half,
                rounding,
            } => Aabb::from_center_half(*center, half.abs() + Vec3::splat(rounding.abs())),
            Node::Cylinder {
                center,
                axis,
                radius,
                half_height,
            } => {
                let axis = normalized_axis(*axis);
                let r = radius.abs();
                let h = half_height.abs();
                let extent = Vec3::new(
                    axis.x.abs() * h + r * (1.0 - axis.x * axis.x).max(0.0).sqrt(),
                    axis.y.abs() * h + r * (1.0 - axis.y * axis.y).max(0.0).sqrt(),
                    axis.z.abs() * h + r * (1.0 - axis.z * axis.z).max(0.0).sqrt(),
                );
                Aabb::from_center_half(*center, extent)
            }
            Node::Capsule { a, b, radius } => Aabb::new(
                a.min(*b) - Vec3::splat(radius.abs()),
                a.max(*b) + Vec3::splat(radius.abs()),
            ),
            Node::Torus {
                center,
                major,
                minor,
                ..
            } => Aabb::from_center_half(*center, Vec3::splat((major.abs() + minor.abs()).max(0.0))),
            Node::Union { a, b } => self.node_bounds(*a).union(self.node_bounds(*b)),
            Node::Intersect { a, b } => self.node_bounds(*a).intersection(self.node_bounds(*b)),
            Node::Subtract { a, .. } => self.node_bounds(*a),
            Node::SmoothUnion { a, b, k } => self
                .node_bounds(*a)
                .union(self.node_bounds(*b))
                .expanded((k.abs() / 4.0).max(0.0)),
            Node::SmoothIntersect { a, b, k } => self
                .node_bounds(*a)
                .intersection(self.node_bounds(*b))
                .expanded((k.abs() / 4.0).max(0.0)),
            Node::SmoothSubtract { a, k, .. } => {
                self.node_bounds(*a).expanded((k.abs() / 4.0).max(0.0))
            }
            Node::Offset { child, distance } => {
                let bounds = self.node_bounds(*child);
                if distance.is_sign_positive() {
                    bounds.expanded(distance.abs())
                } else {
                    bounds
                }
            }
            Node::Transform { child, transform } => {
                transform.transform_aabb(self.node_bounds(*child))
            }
        }
    }

    pub fn category(&self) -> FieldCategory {
        match self.root {
            Some(root) => self.node_category(root),
            None => FieldCategory::ExactSdf,
        }
    }

    fn node_category(&self, id: NodeId) -> FieldCategory {
        match &self.nodes[id.0 as usize] {
            Node::Sphere { .. }
            | Node::Box { .. }
            | Node::Cylinder { .. }
            | Node::Capsule { .. }
            | Node::Torus { .. } => FieldCategory::ExactSdf,
            Node::Union { a, b } | Node::Intersect { a, b } | Node::Subtract { a, b } => {
                combine_category(self.node_category(*a), self.node_category(*b))
            }
            Node::SmoothUnion { a, b, .. }
            | Node::SmoothIntersect { a, b, .. }
            | Node::SmoothSubtract { a, b, .. } => {
                combine_category(self.node_category(*a), self.node_category(*b))
            }
            Node::Offset { child, distance } => {
                if distance.abs() <= f32::EPSILON {
                    self.node_category(*child)
                } else {
                    FieldCategory::ApproximateSdf
                }
            }
            Node::Transform { child, .. } => self.node_category(*child),
        }
    }
}

fn combine_category(a: FieldCategory, b: FieldCategory) -> FieldCategory {
    match (a, b) {
        (FieldCategory::GeneralField, _) | (_, FieldCategory::GeneralField) => {
            FieldCategory::GeneralField
        }
        _ => FieldCategory::ApproximateSdf,
    }
}

fn shift_primitive(node: &mut Node, offset: Vec3) {
    match node {
        Node::Sphere { center, .. }
        | Node::Box { center, .. }
        | Node::Cylinder { center, .. }
        | Node::Torus { center, .. } => {
            *center += offset;
        }
        Node::Capsule { a, b, .. } => {
            *a += offset;
            *b += offset;
        }
        _ => {}
    }
}

fn rebase_node(
    nodes: &mut [Node],
    id: NodeId,
    offset: Vec3,
    world_frame: bool,
    visited: &mut HashSet<NodeId>,
) {
    if !visited.insert(id) {
        return;
    }

    if matches!(&nodes[id.0 as usize], Node::Transform { .. }) {
        if world_frame {
            if let Node::Transform { transform, .. } = &mut nodes[id.0 as usize] {
                transform.translation += offset;
            }
        }
        if let Node::Transform { child, .. } = &nodes[id.0 as usize] {
            let child = *child;
            rebase_node(nodes, child, offset, false, visited);
        }
        return;
    }

    if world_frame {
        shift_primitive(&mut nodes[id.0 as usize], offset);
    }

    let children: Vec<NodeId> = nodes[id.0 as usize]
        .children()
        .into_iter()
        .flatten()
        .collect();
    for child in children {
        rebase_node(nodes, child, offset, world_frame, visited);
    }
}

fn normalize_or_none(value: Vec3) -> Option<Vec3> {
    let length = value.length();
    if length > 1e-9 && length.is_finite() {
        Some(value / length)
    } else {
        None
    }
}

fn smooth_blend(a: f32, b: f32, k: f32, max_mode: bool) -> f32 {
    if k <= 0.0 {
        let picks_a = if max_mode { a > b } else { a < b };
        return if picks_a { 0.0 } else { 1.0 };
    }
    let h = if max_mode {
        0.5 + 0.5 * (a - b) / k
    } else {
        0.5 + 0.5 * (b - a) / k
    };
    h.clamp(0.0, 1.0)
}

fn grad_round_box(point: Vec3, center: Vec3, half: Vec3, rounding: f32) -> Option<Vec3> {
    let local = point - center;
    let sign = Vec3::new(local.x.signum(), local.y.signum(), local.z.signum());
    let rounding = rounding.max(0.0);
    let q = local.abs() - half.abs() + Vec3::splat(rounding);

    if q.max_element() > 0.0 {
        let normal = normalize_or_none(q.max(Vec3::ZERO))?;
        let gradient = normal * sign;
        if gradient.length_squared() > 0.25 {
            Some(gradient)
        } else {
            None
        }
    } else {
        let maximum = q.max_element();
        let mut gradient = Vec3::ZERO;
        if (q.x - maximum).abs() <= 1e-6 {
            gradient.x = sign.x;
        }
        if (q.y - maximum).abs() <= 1e-6 {
            gradient.y = sign.y;
        }
        if (q.z - maximum).abs() <= 1e-6 {
            gradient.z = sign.z;
        }
        normalize_or_none(gradient)
    }
}

fn grad_cylinder(
    point: Vec3,
    center: Vec3,
    axis: Vec3,
    radius: f32,
    half_height: f32,
) -> Option<Vec3> {
    let axis = normalized_axis(axis);
    let q = point - center;
    let z = q.dot(axis);
    let radial = q - axis * z;
    let radial_length = radial.length();
    let radial_direction = if radial_length > 1e-9 {
        radial / radial_length
    } else {
        Vec3::ZERO
    };

    let d_radial = radial_length - radius.abs();
    let d_axis = z.abs() - half_height.abs();

    if d_radial <= 0.0 && d_axis <= 0.0 {
        if d_radial > d_axis {
            if radial_length > 1e-9 {
                Some(radial_direction)
            } else {
                None
            }
        } else {
            Some(axis * if z >= 0.0 { 1.0 } else { -1.0 })
        }
    } else {
        let radial_out = d_radial.max(0.0);
        let axis_out = d_axis.max(0.0);
        let length = (radial_out * radial_out + axis_out * axis_out).sqrt();
        if length <= 1e-9 {
            return None;
        }
        let axis_sign = if z >= 0.0 { 1.0 } else { -1.0 };
        Some((radial_direction * radial_out + axis * (axis_sign * axis_out)) / length)
    }
}

fn grad_torus(point: Vec3, center: Vec3, normal: Vec3, major: f32, _minor: f32) -> Option<Vec3> {
    let normal = normalized_axis(normal);
    let q = point - center;
    let z = q.dot(normal);
    let radial = q - normal * z;
    let radial_length = radial.length();
    let radial_direction = if radial_length > 1e-9 {
        radial / radial_length
    } else {
        Vec3::ZERO
    };

    let d = Vec2::new(radial_length - major.abs(), z);
    let length = d.length();
    if length <= 1e-9 {
        return None;
    }

    let gradient = (radial_direction * d.x + normal * d.y) / length;
    if gradient.length_squared() > 0.5 {
        Some(gradient)
    } else {
        None
    }
}

fn normalized_axis(axis: Vec3) -> Vec3 {
    let axis = axis.normalize_or_zero();
    if axis.length_squared() > 0.5 {
        axis
    } else {
        Vec3::Z
    }
}

#[inline]
fn sd_round_box(point: Vec3, center: Vec3, half: Vec3, rounding: f32) -> f32 {
    let r = rounding.max(0.0);
    let q = (point - center).abs() - half.abs() + Vec3::splat(r);
    q.max(Vec3::ZERO).length() + q.x.max(q.y).max(q.z).min(0.0) - r
}

#[inline]
fn sd_cylinder(point: Vec3, center: Vec3, axis: Vec3, radius: f32, half_height: f32) -> f32 {
    let axis = normalized_axis(axis);
    let q = point - center;
    let z = q.dot(axis);
    let radial = (q - axis * z).length();
    let d = Vec2::new(radial - radius.abs(), z.abs() - half_height.abs());
    d.x.max(d.y).min(0.0) + d.max(Vec2::ZERO).length()
}

#[inline]
fn sd_capsule(point: Vec3, a: Vec3, b: Vec3, radius: f32) -> f32 {
    let pa = point - a;
    let ba = b - a;
    let denom = ba.length_squared();
    let h = if denom > 1e-12 {
        (pa.dot(ba) / denom).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (pa - ba * h).length() - radius.abs()
}

#[inline]
fn sd_torus(point: Vec3, center: Vec3, normal: Vec3, major: f32, minor: f32) -> f32 {
    let normal = normalized_axis(normal);
    let q = point - center;
    let z = q.dot(normal);
    let radial = (q - normal * z).length();
    Vec2::new(radial - major.abs(), z).length() - minor.abs()
}

fn dvec3(value: Vec3) -> DVec3 {
    DVec3::new(value.x as f64, value.y as f64, value.z as f64)
}

fn normalized_axis_f64(axis: DVec3) -> DVec3 {
    let axis = axis.normalize_or_zero();
    if axis.length_squared() > 0.5 {
        axis
    } else {
        DVec3::Z
    }
}

fn rotation_f64(transform: Transform) -> DQuat {
    DQuat::from_xyzw(
        transform.rotation.x as f64,
        transform.rotation.y as f64,
        transform.rotation.z as f64,
        transform.rotation.w as f64,
    )
    .normalize()
}

fn inverse_point_f64(transform: Transform, point: DVec3) -> DVec3 {
    rotation_f64(transform).conjugate() * (point - dvec3(transform.translation))
        / transform.scale as f64
}

fn normalize_or_none_f64(value: DVec3) -> Option<DVec3> {
    let length = value.length();
    if length > 1e-12 && length.is_finite() {
        Some(value / length)
    } else {
        None
    }
}

fn grad_round_box_f64(point: DVec3, center: DVec3, half: DVec3, rounding: f64) -> Option<DVec3> {
    let local = point - center;
    let sign = DVec3::new(local.x.signum(), local.y.signum(), local.z.signum());
    let rounding = rounding.max(0.0);
    let q = local.abs() - half.abs() + DVec3::splat(rounding);
    if q.max_element() > 0.0 {
        let normal = normalize_or_none_f64(q.max(DVec3::ZERO))?;
        let gradient = normal * sign;
        if gradient.length_squared() > 0.25 {
            Some(gradient)
        } else {
            None
        }
    } else {
        let maximum = q.max_element();
        let mut gradient = DVec3::ZERO;
        if (q.x - maximum).abs() <= 1e-12 {
            gradient.x = sign.x;
        }
        if (q.y - maximum).abs() <= 1e-12 {
            gradient.y = sign.y;
        }
        if (q.z - maximum).abs() <= 1e-12 {
            gradient.z = sign.z;
        }
        normalize_or_none_f64(gradient)
    }
}

fn grad_cylinder_f64(
    point: DVec3,
    center: DVec3,
    axis: DVec3,
    radius: f64,
    half_height: f64,
) -> Option<DVec3> {
    let axis = normalized_axis_f64(axis);
    let q = point - center;
    let z = q.dot(axis);
    let radial = q - axis * z;
    let radial_length = radial.length();
    let radial_direction = if radial_length > 1e-12 {
        radial / radial_length
    } else {
        DVec3::ZERO
    };
    let d_radial = radial_length - radius.abs();
    let d_axis = z.abs() - half_height.abs();
    if d_radial <= 0.0 && d_axis <= 0.0 {
        if d_radial > d_axis {
            if radial_length > 1e-12 {
                Some(radial_direction)
            } else {
                None
            }
        } else {
            Some(axis * if z >= 0.0 { 1.0 } else { -1.0 })
        }
    } else {
        let radial_out = d_radial.max(0.0);
        let axis_out = d_axis.max(0.0);
        let length = (radial_out * radial_out + axis_out * axis_out).sqrt();
        if length <= 1e-12 {
            return None;
        }
        let axis_sign = if z >= 0.0 { 1.0 } else { -1.0 };
        Some((radial_direction * radial_out + axis * (axis_sign * axis_out)) / length)
    }
}

fn grad_torus_f64(
    point: DVec3,
    center: DVec3,
    normal: DVec3,
    major: f64,
    _minor: f64,
) -> Option<DVec3> {
    let normal = normalized_axis_f64(normal);
    let q = point - center;
    let z = q.dot(normal);
    let radial = q - normal * z;
    let radial_length = radial.length();
    let radial_direction = if radial_length > 1e-12 {
        radial / radial_length
    } else {
        DVec3::ZERO
    };
    let d = DVec2::new(radial_length - major.abs(), z);
    let length = d.length();
    if length <= 1e-12 {
        return None;
    }
    Some((radial_direction * d.x + normal * d.y) / length)
}

fn smooth_blend_f64(a: f64, b: f64, k: f64, max_mode: bool) -> f64 {
    if k <= 0.0 {
        let picks_a = if max_mode { a > b } else { a < b };
        return if picks_a { 0.0 } else { 1.0 };
    }
    let h = if max_mode {
        0.5 + 0.5 * (a - b) / k
    } else {
        0.5 + 0.5 * (b - a) / k
    };
    h.clamp(0.0, 1.0)
}

fn sd_round_box_f64(point: DVec3, center: DVec3, half: DVec3, rounding: f64) -> f64 {
    let r = rounding.max(0.0);
    let q = (point - center).abs() - half.abs() + DVec3::splat(r);
    q.max(DVec3::ZERO).length() + q.x.max(q.y).max(q.z).min(0.0) - r
}

fn sd_cylinder_f64(point: DVec3, center: DVec3, axis: DVec3, radius: f64, half_height: f64) -> f64 {
    let axis = normalized_axis_f64(axis);
    let q = point - center;
    let z = q.dot(axis);
    let radial = (q - axis * z).length();
    let d = DVec2::new(radial - radius.abs(), z.abs() - half_height.abs());
    d.x.max(d.y).min(0.0) + d.max(DVec2::ZERO).length()
}

fn sd_capsule_f64(point: DVec3, a: DVec3, b: DVec3, radius: f64) -> f64 {
    let pa = point - a;
    let ba = b - a;
    let denom = ba.length_squared();
    let h = if denom > 1e-24 {
        (pa.dot(ba) / denom).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (pa - ba * h).length() - radius.abs()
}

fn sd_torus_f64(point: DVec3, center: DVec3, normal: DVec3, major: f64, minor: f64) -> f64 {
    let normal = normalized_axis_f64(normal);
    let q = point - center;
    let z = q.dot(normal);
    let radial = (q - normal * z).length();
    DVec2::new(radial - major.abs(), z).length() - minor.abs()
}

fn smin_f64(a: f64, b: f64, k: f64) -> f64 {
    if k <= 0.0 {
        a.min(b)
    } else {
        let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
        b + (a - b) * h - k * h * (1.0 - h)
    }
}

fn smax_f64(a: f64, b: f64, k: f64) -> f64 {
    -smin_f64(-a, -b, k)
}

#[inline]
pub fn smin(a: f32, b: f32, k: f32) -> f32 {
    if k <= 0.0 {
        a.min(b)
    } else {
        let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
        b + (a - b) * h - k * h * (1.0 - h)
    }
}

#[inline]
pub fn smax(a: f32, b: f32, k: f32) -> f32 {
    -smin(-a, -b, k)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sphere_graph(radius: f32) -> FieldGraph {
        let mut graph = FieldGraph::new();
        let sphere = graph.sphere(Vec3::ZERO, radius);
        graph.set_root(sphere);
        graph
    }

    #[test]
    fn f64_reference_keeps_precision_at_world_scale() {
        let center = Vec3::new(1_000_000.0, 200_000.0, -300_000.0);
        let mut graph = FieldGraph::new();
        let root = graph.sphere(center, 1.0);
        graph.set_root(root);

        let point = DVec3::new(
            center.x as f64 + 0.03,
            center.y as f64 - 0.04,
            center.z as f64 + 0.05,
        );
        let expected = (point - dvec3(center)).length() - 1.0;
        let value = graph.evaluate_f64(point);
        assert!(
            (value - expected).abs() < 1e-10,
            "f64 value {value} vs {expected}"
        );

        let f32_point = Vec3::new(point.x as f32, point.y as f32, point.z as f32);
        let f32_error = (graph.evaluate(f32_point) as f64 - expected).abs();
        assert!(
            f32_error > 1e-3,
            "expected f32 world-scale error, got {f32_error}"
        );

        let gradient = graph.gradient_f64(point);
        let direction = (point - dvec3(center)).normalize();
        assert!((gradient - direction).length() < 1e-4);
    }

    #[test]
    fn f64_analytic_gradient_matches_central_difference() {
        let graph = crate::presets::bracket();
        for point in [
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(20.0, 10.0, 5.0),
            Vec3::new(-17.0, -9.0, 1.0),
            Vec3::new(5.0, 15.0, -3.0),
        ] {
            let point = dvec3(point);
            let analytic = graph
                .gradient_f64_analytic(point)
                .expect("analytic gradient");
            let h = 1e-7;
            let dx = DVec3::X * h;
            let dy = DVec3::Y * h;
            let dz = DVec3::Z * h;
            let denominator = 2.0 * h;
            let numerical = DVec3::new(
                (graph.evaluate_f64(point + dx) - graph.evaluate_f64(point - dx)) / denominator,
                (graph.evaluate_f64(point + dy) - graph.evaluate_f64(point - dy)) / denominator,
                (graph.evaluate_f64(point + dz) - graph.evaluate_f64(point - dz)) / denominator,
            );
            let error = (analytic - numerical).length();
            assert!(error < 1e-5, "gradient error {error} at {point:?}");
        }
    }

    #[test]
    fn f64_csg_matches_f32_at_moderate_scale() {
        let graph = crate::presets::bracket();
        for point in [
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(20.0, 10.0, 5.0),
            Vec3::new(-17.0, -9.0, 1.0),
            Vec3::new(5.0, 15.0, -3.0),
        ] {
            let f32_value = graph.evaluate(point);
            let f64_value = graph.evaluate_f64(dvec3(point));
            assert!(
                (f64_value - f32_value as f64).abs() < 1e-3,
                "value at {point:?}: {f32_value} vs {f64_value}"
            );
            let f32_gradient = graph.gradient(point);
            let f64_gradient = graph.gradient_f64(dvec3(point));
            assert!(
                (f64_gradient - dvec3(f32_gradient)).length() < 1e-2,
                "gradient at {point:?}: {f32_gradient} vs {f64_gradient}"
            );
        }
    }

    #[test]
    fn sphere_distance_is_exact() {
        let graph = sphere_graph(2.0);
        assert!((graph.evaluate(Vec3::ZERO) + 2.0).abs() < 1e-6);
        assert!(graph.evaluate(Vec3::new(2.0, 0.0, 0.0)).abs() < 1e-6);
        assert!((graph.evaluate(Vec3::new(4.0, 0.0, 0.0)) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn box_rounding_changes_corner_distance() {
        let mut graph = FieldGraph::new();
        let sharp = graph.rounded_box(Vec3::ZERO, Vec3::splat(1.0), 0.0);
        let root = sharp;
        graph.set_root(root);
        let corner = Vec3::splat(1.0);
        let sharp_value = graph.evaluate(corner);

        let mut rounded = FieldGraph::new();
        let node = rounded.rounded_box(Vec3::ZERO, Vec3::splat(1.0), 0.25);
        rounded.set_root(node);
        let rounded_value = rounded.evaluate(corner);

        assert!(sharp_value.abs() < 1e-6);
        assert!(rounded_value > 0.0);
    }

    #[test]
    fn compacted_graph_keeps_shared_children_and_drops_orphans() {
        let mut graph = FieldGraph::new();
        let sphere = graph.sphere(Vec3::ZERO, 1.0);
        let union = graph.union(sphere, sphere);
        let orphan = graph.rounded_box(Vec3::splat(3.0), Vec3::splat(0.25), 0.0);
        graph.set_root(union);

        let (compacted, remap) = graph.compacted(&[union]);
        assert_eq!(compacted.len(), 2);
        assert_eq!(remap[orphan.0 as usize], None);
        let new_union = remap[union.0 as usize].expect("root is reachable");
        assert_eq!(compacted.root(), Some(new_union));
        match compacted.node(new_union).expect("union was copied") {
            Node::Union { a, b } => assert_eq!(a, b, "shared child must stay shared"),
            other => panic!("expected union, got {:?}", other.op_name()),
        }

        let point = Vec3::new(0.25, 0.0, 0.0);
        assert!(
            (graph.evaluate_at(union, point) - compacted.evaluate_at(new_union, point)).abs()
                < 1e-6
        );
    }

    #[test]
    fn union_is_min_and_subtract_flips() {
        let mut graph = FieldGraph::new();
        let sphere = graph.sphere(Vec3::ZERO, 1.0);
        let box_node = graph.rounded_box(Vec3::splat(0.5), Vec3::splat(0.75), 0.0);
        let union = graph.union(sphere, box_node);
        graph.set_root(union);

        let p = Vec3::new(0.5, 0.0, 0.0);
        let mut sphere_graph = graph.clone();
        sphere_graph.set_root(sphere);
        let mut box_graph = graph.clone();
        box_graph.set_root(box_node);

        let expected = sphere_graph.evaluate(p).min(box_graph.evaluate(p));
        assert!((graph.evaluate(p) - expected).abs() < 1e-6);

        let cut = graph.subtract(sphere, box_node);
        graph.set_root(cut);
        let expected = sphere_graph.evaluate(p).max(-box_graph.evaluate(p));
        assert!((graph.evaluate(p) - expected).abs() < 1e-6);
    }

    #[test]
    fn smooth_union_is_below_hard_union() {
        let mut graph = FieldGraph::new();
        let a = graph.sphere(Vec3::new(-0.5, 0.0, 0.0), 1.0);
        let b = graph.sphere(Vec3::new(0.5, 0.0, 0.0), 1.0);
        let hard = graph.union(a, b);
        let smooth = graph.smooth_union(a, b, 0.5);

        let p = Vec3::ZERO;
        let hard_value = {
            let mut g = graph.clone();
            g.set_root(hard);
            g.evaluate(p)
        };
        graph.set_root(smooth);
        assert!(graph.evaluate(p) < hard_value);
    }

    #[test]
    fn transform_moves_sphere() {
        let mut graph = FieldGraph::new();
        let sphere = graph.sphere(Vec3::ZERO, 1.0);
        let moved = graph.transform(
            sphere,
            Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)),
        );
        graph.set_root(moved);
        assert!((graph.evaluate(Vec3::new(3.0, 0.0, 0.0)) + 1.0).abs() < 1e-6);
        assert!((graph.evaluate(Vec3::ZERO) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn bounds_track_transform_and_smoothing() {
        let mut graph = FieldGraph::new();
        let sphere = graph.sphere(Vec3::ZERO, 1.0);
        let moved = graph.transform(
            sphere,
            Transform::from_translation(Vec3::new(5.0, 0.0, 0.0)),
        );
        graph.set_root(moved);
        let bounds = graph.bounds();
        assert!((bounds.min.x - 4.0).abs() < 1e-6);
        assert!((bounds.max.x - 6.0).abs() < 1e-6);
    }

    #[test]
    fn serde_roundtrip_preserves_structure() {
        let mut graph = FieldGraph::new();
        let sphere = graph.sphere(Vec3::ZERO, 1.0);
        let box_node = graph.rounded_box(Vec3::new(1.0, 0.0, 0.0), Vec3::splat(0.5), 0.1);
        let root = graph.subtract(box_node, sphere);
        graph.set_root(root);

        let json = serde_json::to_string(&graph).unwrap();
        let restored: FieldGraph = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.len(), graph.len());
        assert_eq!(restored.root(), graph.root());
        assert!((restored.evaluate(Vec3::ZERO) - graph.evaluate(Vec3::ZERO)).abs() < 1e-6);
    }

    fn assert_gradient_close(graph: &FieldGraph, point: Vec3, tolerance: f32) {
        let analytic = graph
            .gradient_analytic(point)
            .unwrap_or_else(|| panic!("no analytic gradient at {point:?}"));
        let numerical = graph.gradient_numerical(point);
        let error = (analytic - numerical).length();
        assert!(
            error <= tolerance,
            "gradient mismatch at {point:?}: analytic={analytic:?} numerical={numerical:?} error={error}"
        );
    }

    #[test]
    fn analytic_gradient_matches_numerical_for_primitives() {
        let mut sphere = FieldGraph::new();
        let root = sphere.sphere(Vec3::ZERO, 1.0);
        sphere.set_root(root);
        assert_gradient_close(&sphere, Vec3::new(1.5, 0.3, 0.2), 5e-3);

        let mut box_graph = FieldGraph::new();
        let root = box_graph.rounded_box(Vec3::ZERO, Vec3::splat(1.0), 0.1);
        box_graph.set_root(root);
        assert_gradient_close(&box_graph, Vec3::new(1.3, 0.4, 0.2), 1e-2);

        let mut cylinder = FieldGraph::new();
        let root = cylinder.cylinder(Vec3::ZERO, Vec3::Z, 1.0, 2.0);
        cylinder.set_root(root);
        assert_gradient_close(&cylinder, Vec3::new(1.4, 0.3, 0.5), 1e-2);

        let mut capsule = FieldGraph::new();
        let root = capsule.capsule(Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), 0.5);
        capsule.set_root(root);
        assert_gradient_close(&capsule, Vec3::new(0.2, 1.0, 0.0), 5e-3);

        let mut torus = FieldGraph::new();
        let root = torus.torus(Vec3::ZERO, Vec3::Z, 1.0, 0.3);
        torus.set_root(root);
        assert_gradient_close(&torus, Vec3::new(1.3, 0.2, 0.4), 1e-2);
    }

    #[test]
    fn analytic_gradient_through_csg_and_transform() {
        let mut graph = FieldGraph::new();
        let sphere = graph.sphere(Vec3::new(-0.4, 0.0, 0.0), 1.0);
        let box_node = graph.rounded_box(Vec3::new(0.5, 0.2, 0.0), Vec3::splat(0.7), 0.1);
        let union = graph.union(sphere, box_node);
        let cutter = graph.sphere(Vec3::new(0.2, 0.2, 0.8), 0.4);
        let cut = graph.subtract(union, cutter);
        let smooth = graph.smooth_union(cut, box_node, 0.2);
        let root = smooth;
        graph.set_root(root);

        assert_gradient_close(&graph, Vec3::new(-1.2, 0.3, 0.2), 2e-2);
        assert_gradient_close(&graph, Vec3::new(1.35, 0.55, 0.25), 2e-2);
        assert_gradient_close(&graph, Vec3::new(0.1, -0.8, 0.1), 2e-2);

        let rotated =
            Transform::new(Vec3::new(2.0, -1.0, 0.5), Quat::from_rotation_z(0.4), 1.0).unwrap();
        let transformed = graph.transform(root, rotated);
        let mut moved = graph.clone();
        moved.set_root(transformed);
        assert_gradient_close(&moved, Vec3::new(2.6, -0.4, 0.8), 3e-2);
    }

    #[test]
    fn smooth_boolean_gradients_match_numerical() {
        let mut graph = FieldGraph::new();
        let a = graph.sphere(Vec3::new(-0.5, 0.0, 0.0), 1.0);
        let b = graph.sphere(Vec3::new(0.5, 0.0, 0.0), 1.0);

        let root = graph.smooth_intersect(a, b, 0.4);
        graph.set_root(root);
        assert_gradient_close(&graph, Vec3::new(0.0, 0.9, 0.0), 2e-2);

        let root = graph.smooth_subtract(a, b, 0.4);
        graph.set_root(root);
        assert_gradient_close(&graph, Vec3::new(-0.9, 0.4, 0.0), 2e-2);
    }

    #[test]
    fn rebased_transform_preserves_values() {
        let mut graph = FieldGraph::new();
        let sphere = graph.sphere(Vec3::new(0.2, -0.1, 0.3), 1.0);
        let transform =
            Transform::new(Vec3::new(0.5, 0.25, -0.1), Quat::from_rotation_y(0.35), 1.0).unwrap();
        let root = graph.transform(sphere, transform);
        graph.set_root(root);

        let reference = Vec3::new(200.0, -100.0, 50.0);
        let rebased = graph.rebased(reference);

        for local in [
            Vec3::new(0.1, 0.2, -0.3),
            Vec3::new(1.1, -0.4, 0.6),
            Vec3::new(-0.5, 0.7, 0.2),
        ] {
            let world = local + reference;
            assert!((graph.evaluate(world) - rebased.evaluate(local)).abs() < 3e-4);
            let before = graph.gradient_analytic(world).unwrap();
            let after = rebased.gradient_analytic(local).unwrap();
            assert!((before - after).length() < 5e-3);
        }
    }

    #[test]
    fn rebased_field_preserves_values_and_gradients() {
        let mut graph = FieldGraph::new();
        let sphere = graph.sphere(Vec3::new(0.5, -0.25, 0.75), 1.0);
        let box_node = graph.rounded_box(Vec3::splat(1.0), Vec3::splat(0.6), 0.1);
        let root = graph.union(sphere, box_node);
        graph.set_root(root);

        let reference = Vec3::new(1000.0, -500.0, 250.0);
        let rebased = graph.rebased(reference);

        for local in [
            Vec3::new(0.2, 0.1, 0.3),
            Vec3::new(1.2, -0.4, 0.6),
            Vec3::new(-0.7, 0.9, 0.1),
        ] {
            let world = local + reference;
            let before = graph.evaluate(world);
            let after = rebased.evaluate(local);
            assert!(
                (before - after).abs() < 2e-4,
                "value mismatch: before={before} after={after}"
            );

            let g_before = graph.gradient_analytic(world).unwrap();
            let g_after = rebased.gradient_analytic(local).unwrap();
            assert!(
                (g_before - g_after).length() < 5e-3,
                "gradient mismatch: before={g_before:?} after={g_after:?}"
            );
        }
    }
}
