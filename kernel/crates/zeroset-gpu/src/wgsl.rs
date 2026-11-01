use std::collections::HashSet;
use std::fmt::Write;

use glam::Mat3;
use zeroset_core::{FieldGraph, Node, NodeId, Vec3};

use crate::GpuError;

const HELPERS: &str = r#"
fn zs_smin(a: f32, b: f32, k: f32) -> f32 {
    if (k <= 0.0) {
        return min(a, b);
    }
    let h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return b + (a - b) * h - k * h * (1.0 - h);
}

fn zs_smax(a: f32, b: f32, k: f32) -> f32 {
    return -zs_smin(-a, -b, k);
}

fn zs_sd_round_box(p: vec3<f32>, c: vec3<f32>, h: vec3<f32>, r: f32) -> f32 {
    let q = abs(p - c) - abs(h) + vec3<f32>(abs(r));
    return length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0) - abs(r);
}

fn zs_sd_cylinder(p: vec3<f32>, c: vec3<f32>, axis: vec3<f32>, radius: f32, half_height: f32) -> f32 {
    let a = normalize(axis);
    let q = p - c;
    let z = dot(q, a);
    let radial = length(q - a * z);
    let d = vec2<f32>(radial - abs(radius), abs(z) - abs(half_height));
    return min(max(d.x, d.y), 0.0) + length(max(d, vec2<f32>(0.0)));
}

fn zs_sd_capsule(p: vec3<f32>, a: vec3<f32>, b: vec3<f32>, radius: f32) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-8), 0.0, 1.0);
    return length(pa - ba * h) - abs(radius);
}

fn zs_sd_torus(p: vec3<f32>, c: vec3<f32>, normal: vec3<f32>, major: f32, minor: f32) -> f32 {
    let n = normalize(normal);
    let q = p - c;
    let z = dot(q, n);
    let radial = length(q - n * z);
    return length(vec2<f32>(radial - abs(major), z)) - abs(minor);
}

fn zs_grad_sphere(p: vec3<f32>, c: vec3<f32>) -> vec3<f32> {
    return normalize(p - c);
}

fn zs_grad_round_box(p: vec3<f32>, c: vec3<f32>, h: vec3<f32>, r: f32) -> vec3<f32> {
    let local = p - c;
    let s = select(vec3<f32>(-1.0), vec3<f32>(1.0), local >= vec3<f32>(0.0));
    let q = abs(local) - abs(h) + vec3<f32>(abs(r));
    if (max(q.x, max(q.y, q.z)) > 0.0) {
        let n = normalize(max(q, vec3<f32>(0.0)));
        return n * s;
    }
    let m = max(q.x, max(q.y, q.z));
    var g = vec3<f32>(0.0);
    if (abs(q.x - m) <= 1e-6) { g.x = s.x; }
    if (abs(q.y - m) <= 1e-6) { g.y = s.y; }
    if (abs(q.z - m) <= 1e-6) { g.z = s.z; }
    return normalize(g);
}

fn zs_grad_cylinder(
    p: vec3<f32>,
    c: vec3<f32>,
    axis: vec3<f32>,
    radius: f32,
    half_height: f32
) -> vec3<f32> {
    let a = normalize(axis);
    let q = p - c;
    let z = dot(q, a);
    let radial = q - a * z;
    let rlen = length(radial);
    var dir = vec3<f32>(0.0);
    if (rlen > 1e-9) {
        dir = radial / rlen;
    }
    let dx = rlen - abs(radius);
    let dy = abs(z) - abs(half_height);
    let axis_sign = select(-1.0, 1.0, z >= 0.0);
    if (dx <= 0.0 && dy <= 0.0) {
        if (dx > dy) {
            return dir;
        }
        return a * axis_sign;
    }
    let ox = max(dx, 0.0);
    let oy = max(dy, 0.0);
    let len = sqrt(ox * ox + oy * oy);
    if (len <= 1e-9) {
        return vec3<f32>(0.0);
    }
    return (dir * ox + a * (axis_sign * oy)) / len;
}

fn zs_grad_capsule(
    p: vec3<f32>,
    a: vec3<f32>,
    b: vec3<f32>,
    radius: f32
) -> vec3<f32> {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-8), 0.0, 1.0);
    return normalize(p - (a + ba * h));
}

fn zs_grad_torus(
    p: vec3<f32>,
    c: vec3<f32>,
    normal: vec3<f32>,
    major: f32,
    minor: f32
) -> vec3<f32> {
    let n = normalize(normal);
    let q = p - c;
    let z = dot(q, n);
    let radial = q - n * z;
    let rlen = length(radial);
    var dir = vec3<f32>(0.0);
    if (rlen > 1e-9) {
        dir = radial / rlen;
    }
    let d = vec2<f32>(rlen - abs(major), z);
    let len = length(d);
    if (len <= 1e-9) {
        return vec3<f32>(0.0);
    }
    return (dir * d.x + n * d.y) / len;
}

fn zs_smin_grad(
    va: f32,
    vb: f32,
    ga: vec3<f32>,
    gb: vec3<f32>,
    k: f32
) -> vec3<f32> {
    if (k <= 0.0) {
        if (va < vb) {
            return ga;
        }
        return gb;
    }
    let h = clamp(0.5 + 0.5 * (vb - va) / k, 0.0, 1.0);
    return ga * h + gb * (1.0 - h);
}

fn zs_smax_grad(
    va: f32,
    vb: f32,
    ga: vec3<f32>,
    gb: vec3<f32>,
    k: f32
) -> vec3<f32> {
    return -zs_smin_grad(-va, -vb, -ga, -gb, k);
}

fn zs_ssub_grad(
    va: f32,
    vb: f32,
    ga: vec3<f32>,
    gb: vec3<f32>,
    k: f32
) -> vec3<f32> {
    return zs_smax_grad(va, -vb, ga, -gb, k);
}
"#;

const MAIN: &str = r#"
struct ZsParams {
    count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var<uniform> zs_params: ZsParams;
@group(0) @binding(1) var<storage, read> zs_positions: array<f32>;
@group(0) @binding(2) var<storage, read_write> zs_values: array<f32>;

@compute @workgroup_size(64)
fn zs_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= zs_params.count) {
        return;
    }
    let p = vec3<f32>(
        zs_positions[i * 3u],
        zs_positions[i * 3u + 1u],
        zs_positions[i * 3u + 2u]
    );
    zs_values[i] = zs_field(p);
}
"#;

const RAYMARCH_COMMON: &str = r#"
struct ZsRayParams {
    camera: vec4<f32>,
    forward: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    size: vec4<f32>,
    trace: vec4<f32>,
    jitter_weight: vec4<f32>,
};

@group(0) @binding(0) var<uniform> zs_ray: ZsRayParams;

fn zs_pack(color: vec3<f32>) -> u32 {
    let c = clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
    let r = u32(c.x * 255.0 + 0.5);
    let g = u32(c.y * 255.0 + 0.5);
    let b = u32(c.z * 255.0 + 0.5);
    return r | (g << 8u) | (b << 16u) | (255u << 24u);
}

fn zs_shade(uv: vec2<f32>, tile_quality: f32) -> vec4<f32> {
    let width = u32(zs_ray.size.x);
    let height = u32(zs_ray.size.y);
    let ndc = uv * 2.0 - vec2<f32>(1.0);
    let aspect = f32(width) / f32(height);
    let jitter = zs_ray.jitter_weight.xy * 2.0 / vec2<f32>(f32(width), f32(height));
    let direction = normalize(
        zs_ray.forward.xyz
        + zs_ray.right.xyz * ((ndc.x + jitter.x) * zs_ray.size.z * aspect)
        + zs_ray.up.xyz * (-(ndc.y + jitter.y) * zs_ray.size.z)
    );

    let quality = clamp(tile_quality, 0.0, 1.0);
    let max_steps = u32(mix(64.0, zs_ray.trace.x, quality));
    let base_epsilon = zs_ray.trace.y * mix(2.0, 1.0, quality);
    let max_distance = zs_ray.trace.z;
    let step_scale = zs_ray.trace.w;
    let pixel_angle = zs_ray.size.w;

    var t = 0.0;
    var hit = false;
    var p = zs_ray.camera.xyz;

    for (var i: u32 = 0u; i < max_steps; i = i + 1u) {
        let distance = zs_field(p);
        // Screen-space LOD: acceptable error grows with one pixel's footprint.
        // Near surfaces stay sharp; distant surfaces need fewer steps.
        let epsilon = max(base_epsilon, t * pixel_angle);
        if (distance < epsilon) {
            hit = true;
            break;
        }
        let step = max(distance * step_scale, epsilon * 0.5);
        t = t + step;
        if (t > max_distance) {
            break;
        }
        p = zs_ray.camera.xyz + direction * t;
    }

    if (hit) {
        let normal = normalize(zs_gradient(p));
        let key_light = normalize(vec3<f32>(0.35, 0.75, 0.45));
        let sky_light = normalize(vec3<f32>(-0.4, 0.5, -0.7));
        let diffuse = max(dot(normal, key_light), 0.0);
        let fill = max(dot(normal, sky_light), 0.0) * 0.25;
        let rim = pow(1.0 - max(dot(normal, -direction), 0.0), 3.0) * 0.35;
        let base = vec3<f32>(0.42, 0.62, 0.92);
        let color = base * (0.16 + 0.84 * diffuse + fill)
            + vec3<f32>(0.45, 0.62, 0.85) * rim;
        let forward_depth = dot(p - zs_ray.camera.xyz, zs_ray.forward.xyz);
        return vec4<f32>(color, forward_depth);
    }

    let horizon = clamp(1.0 - uv.y, 0.0, 1.0);
    let background = mix(
        vec3<f32>(0.025, 0.04, 0.075),
        vec3<f32>(0.09, 0.13, 0.20),
        horizon
    );
    return vec4<f32>(background, -1.0);
}
"#;

const RAYMARCH_STORAGE_MAIN: &str = r#"
@group(0) @binding(1) var<storage, read_write> zs_pixels: array<u32>;

@compute @workgroup_size(8, 8)
fn zs_render(@builtin(global_invocation_id) gid: vec3<u32>) {
    let width = u32(zs_ray.size.x);
    let height = u32(zs_ray.size.y);
    if (gid.x >= width || gid.y >= height) {
        return;
    }
    let uv = (vec2<f32>(gid.xy) + vec2<f32>(0.5)) / vec2<f32>(f32(width), f32(height));
    let result = zs_shade(uv, 1.0);
    zs_pixels[gid.y * width + gid.x] = zs_pack(result.rgb);
}
"#;

const RAYMARCH_TEXTURE_MAIN: &str = r#"
@group(0) @binding(1) var zs_target: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var<storage, read> zs_tile_quality: array<f32>;

@compute @workgroup_size(8, 8)
fn zs_render(@builtin(global_invocation_id) gid: vec3<u32>) {
    let width = u32(zs_ray.size.x);
    let height = u32(zs_ray.size.y);
    if (gid.x >= width || gid.y >= height) {
        return;
    }
    let coordinate = vec2<i32>(i32(gid.x), i32(gid.y));
    let tiles_x = (width + 15u) / 16u;
    let tile_index = (gid.y / 16u) * tiles_x + (gid.x / 16u);
    let quality = zs_tile_quality[tile_index];
    let uv = (vec2<f32>(gid.xy) + vec2<f32>(0.5)) / vec2<f32>(f32(width), f32(height));
    textureStore(zs_target, coordinate, zs_shade(uv, quality));
}
"#;

const TAA_WGSL: &str = r#"
struct ZsRayParams {
    camera: vec4<f32>,
    forward: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    size: vec4<f32>,
    trace: vec4<f32>,
    jitter_weight: vec4<f32>,
};

struct ZsTaaPrev {
    camera: vec4<f32>,
    forward: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    size: vec4<f32>,
    blend: vec4<f32>,
};

@group(0) @binding(0) var<uniform> zs_ray: ZsRayParams;
@group(0) @binding(1) var zs_current: texture_2d<f32>;
@group(0) @binding(2) var zs_history: texture_2d<f32>;
@group(0) @binding(3) var zs_output: texture_storage_2d<rgba16float, write>;
@group(0) @binding(4) var<uniform> zs_prev: ZsTaaPrev;
@group(0) @binding(5) var zs_sampler: sampler;

@compute @workgroup_size(8, 8)
fn zs_taa(@builtin(global_invocation_id) gid: vec3<u32>) {
    let width = u32(zs_ray.size.x);
    let height = u32(zs_ray.size.y);
    if (gid.x >= width || gid.y >= height) {
        return;
    }
    let coordinate = vec2<i32>(i32(gid.x), i32(gid.y));
    let current = textureLoad(zs_current, coordinate, 0);
    if (current.a <= 0.0) {
        textureStore(zs_output, coordinate, current);
        return;
    }

    let uv = (vec2<f32>(gid.xy) + vec2<f32>(0.5)) / vec2<f32>(f32(width), f32(height));
    let ndc = uv * 2.0 - vec2<f32>(1.0);
    let aspect = f32(width) / f32(height);
    let jitter = zs_ray.jitter_weight.xy * 2.0 / vec2<f32>(f32(width), f32(height));
    let direction = normalize(
        zs_ray.forward.xyz
        + zs_ray.right.xyz * ((ndc.x + jitter.x) * zs_ray.size.z * aspect)
        + zs_ray.up.xyz * (-(ndc.y + jitter.y) * zs_ray.size.z)
    );

    let cos_angle = max(dot(direction, zs_ray.forward.xyz), 1e-4);
    let world = zs_ray.camera.xyz + direction * (current.a / cos_angle);

    var previous = current;
    var alpha = 1.0;
    if (zs_prev.size.w > 0.5) {
        let relative = world - zs_prev.camera.xyz;
        let x = dot(relative, zs_prev.right.xyz);
        let y = dot(relative, zs_prev.up.xyz);
        let z = dot(relative, zs_prev.forward.xyz);
        if (z > 1e-4) {
            let tan_half = zs_prev.size.z;
            let aspect_prev = zs_prev.size.x / zs_prev.size.y;
            let uv_prev = vec2<f32>(
                0.5 + 0.5 * x / (z * tan_half * aspect_prev),
                0.5 - 0.5 * y / (z * tan_half)
            );
            if (all(uv_prev >= vec2<f32>(0.0)) && all(uv_prev <= vec2<f32>(1.0))) {
                let history = textureSampleLevel(zs_history, zs_sampler, uv_prev, 0.0);
                let tolerance = max(0.02 * z, 0.05);
                if (history.a > 0.0 && abs(history.a - z) < tolerance) {
                    previous = history;
                    alpha = zs_prev.blend.x;
                }
            }
        }
    }

    let color = mix(previous.rgb, current.rgb, alpha);
    textureStore(zs_output, coordinate, vec4<f32>(color, current.a));
}
"#;

/// Build only the field/gradient WGSL functions, without a compute entry point.
///
/// This is the reusable half for future renderers, probes, and custom passes.
pub fn compile_field_functions(graph: &FieldGraph) -> Result<String, GpuError> {
    let root = graph.root().ok_or(GpuError::EmptyGraph)?;
    let mut emitter = Emitter {
        graph,
        emitted: HashSet::new(),
        grad_emitted: HashSet::new(),
        functions: String::new(),
        grad_functions: String::new(),
    };
    let root_function = emitter.emit(root)?;
    let root_gradient = emitter.emit_grad(root)?;

    let mut shader = String::new();
    shader.push_str(HELPERS);
    shader.push_str(&emitter.functions);
    shader.push_str(&emitter.grad_functions);
    writeln!(
        shader,
        "fn zs_field(p: vec3<f32>) -> f32 {{ return {root_function}(p); }}"
    )
    .expect("writing to String cannot fail");
    writeln!(
        shader,
        r#"
// Analytic per-node gradient; falls back to central differences only at
// singularities (zero-length gradients).
fn zs_gradient(p: vec3<f32>) -> vec3<f32> {{
    let analytic = {root_gradient}(p);
    if (length(analytic) > 1e-8) {{
        return analytic;
    }}
    let h = 1e-3;
    let dx = vec3<f32>(h, 0.0, 0.0);
    let dy = vec3<f32>(0.0, h, 0.0);
    let dz = vec3<f32>(0.0, 0.0, h);
    let denominator = 2.0 * h;
    return vec3<f32>(
        (zs_field(p + dx) - zs_field(p - dx)) / denominator,
        (zs_field(p + dy) - zs_field(p - dy)) / denominator,
        (zs_field(p + dz) - zs_field(p - dz)) / denominator
    );
}}
"#
    )
    .expect("writing to String cannot fail");
    Ok(shader)
}

/// Compile a field DAG to a WGSL module with a `zs_main` compute entry point.
///
/// Each node becomes a function of `p`, emitted in post-order so children are
/// declared before parents. This makes transforms straightforward: a transform
/// node calls its child with the inverse-mapped point.
pub fn compile_shader(graph: &FieldGraph) -> Result<String, GpuError> {
    let mut shader = compile_field_functions(graph)?;
    shader.push_str(MAIN);
    Ok(shader)
}

/// Compile a field DAG to a full-screen raymarch compute shader that writes a
/// packed RGBA8 buffer (headless path).
pub fn compile_raymarch_shader(graph: &FieldGraph) -> Result<String, GpuError> {
    let mut shader = compile_field_functions(graph)?;
    shader.push_str(RAYMARCH_COMMON);
    shader.push_str(RAYMARCH_STORAGE_MAIN);
    Ok(shader)
}

/// Compile a field DAG to a full-screen raymarch compute shader that writes a
/// `rgba16float` storage texture; the alpha channel stores forward depth.
pub fn compile_raymarch_texture_shader(graph: &FieldGraph) -> Result<String, GpuError> {
    let mut shader = compile_field_functions(graph)?;
    shader.push_str(RAYMARCH_COMMON);
    shader.push_str(RAYMARCH_TEXTURE_MAIN);
    Ok(shader)
}

/// Temporal reprojection pass: current color/depth + history + previous camera.
pub fn compile_taa_shader() -> String {
    TAA_WGSL.to_string()
}

struct Emitter<'a> {
    graph: &'a FieldGraph,
    emitted: HashSet<NodeId>,
    grad_emitted: HashSet<NodeId>,
    functions: String,
    grad_functions: String,
}

impl Emitter<'_> {
    fn emit(&mut self, id: NodeId) -> Result<String, GpuError> {
        let name = format!("zs_node_{}", id.0);
        if self.emitted.contains(&id) {
            return Ok(name);
        }

        let node = self
            .graph
            .node(id)
            .cloned()
            .ok_or(GpuError::InvalidNode(id.0))?;

        let body = match &node {
            Node::Sphere { center, radius } => {
                format!("length(p - {}) - {}", vec3(*center), number(*radius, id)?)
            }
            Node::Box {
                center,
                half,
                rounding,
            } => format!(
                "zs_sd_round_box(p, {}, {}, {})",
                vec3(*center),
                vec3(*half),
                number(*rounding, id)?
            ),
            Node::Cylinder {
                center,
                axis,
                radius,
                half_height,
            } => format!(
                "zs_sd_cylinder(p, {}, {}, {}, {})",
                vec3(*center),
                vec3(normalized_axis(*axis)),
                number(*radius, id)?,
                number(*half_height, id)?
            ),
            Node::Capsule { a, b, radius } => format!(
                "zs_sd_capsule(p, {}, {}, {})",
                vec3(*a),
                vec3(*b),
                number(*radius, id)?
            ),
            Node::Torus {
                center,
                normal,
                major,
                minor,
            } => format!(
                "zs_sd_torus(p, {}, {}, {}, {})",
                vec3(*center),
                vec3(normalized_axis(*normal)),
                number(*major, id)?,
                number(*minor, id)?
            ),
            Node::Union { a, b } => {
                let a = self.emit(*a)?;
                let b = self.emit(*b)?;
                format!("min({a}(p), {b}(p))")
            }
            Node::Intersect { a, b } => {
                let a = self.emit(*a)?;
                let b = self.emit(*b)?;
                format!("max({a}(p), {b}(p))")
            }
            Node::Subtract { a, b } => {
                let a = self.emit(*a)?;
                let b = self.emit(*b)?;
                format!("max({a}(p), -{b}(p))")
            }
            Node::SmoothUnion { a, b, k } => {
                let a = self.emit(*a)?;
                let b = self.emit(*b)?;
                format!("zs_smin({a}(p), {b}(p), {})", number(*k, id)?)
            }
            Node::SmoothIntersect { a, b, k } => {
                let a = self.emit(*a)?;
                let b = self.emit(*b)?;
                format!("zs_smax({a}(p), {b}(p), {})", number(*k, id)?)
            }
            Node::SmoothSubtract { a, b, k } => {
                let a = self.emit(*a)?;
                let b = self.emit(*b)?;
                format!("zs_smax({a}(p), -{b}(p), {})", number(*k, id)?)
            }
            Node::Offset { child, distance } => {
                let child = self.emit(*child)?;
                format!("{child}(p) - {}", number(*distance, id)?)
            }
            Node::Transform { child, transform } => {
                let child = self.emit(*child)?;
                let matrix = Mat3::from_quat(transform.rotation).transpose();
                let columns = matrix.to_cols_array();
                let mat = format!(
                    "mat3x3<f32>({},{},{},{},{},{},{},{},{})",
                    number(columns[0], id)?,
                    number(columns[1], id)?,
                    number(columns[2], id)?,
                    number(columns[3], id)?,
                    number(columns[4], id)?,
                    number(columns[5], id)?,
                    number(columns[6], id)?,
                    number(columns[7], id)?,
                    number(columns[8], id)?
                );
                let scale = number(transform.scale, id)?;
                format!(
                    "{child}({mat} * (p - {}) / {scale}) * {scale}",
                    vec3(transform.translation)
                )
            }
        };

        writeln!(
            self.functions,
            "fn {name}(p: vec3<f32>) -> f32 {{ return {body}; }}"
        )
        .expect("writing to String cannot fail");
        self.emitted.insert(id);
        Ok(name)
    }

    fn emit_grad(&mut self, id: NodeId) -> Result<String, GpuError> {
        let name = format!("zs_grad_{}", id.0);
        if self.grad_emitted.contains(&id) {
            return Ok(name);
        }

        // Ensure the value function (and all children) exist before emitting the
        // gradient function that branches on those values.
        let _ = self.emit(id)?;
        let node = self
            .graph
            .node(id)
            .cloned()
            .ok_or(GpuError::InvalidNode(id.0))?;

        let body = match &node {
            Node::Sphere { center, .. } => {
                format!("return zs_grad_sphere(p, {});", vec3(*center))
            }
            Node::Box {
                center,
                half,
                rounding,
            } => format!(
                "return zs_grad_round_box(p, {}, {}, {});",
                vec3(*center),
                vec3(*half),
                number(*rounding, id)?
            ),
            Node::Cylinder {
                center,
                axis,
                radius,
                half_height,
            } => format!(
                "return zs_grad_cylinder(p, {}, {}, {}, {});",
                vec3(*center),
                vec3(normalized_axis(*axis)),
                number(*radius, id)?,
                number(*half_height, id)?
            ),
            Node::Capsule { a, b, radius } => format!(
                "return zs_grad_capsule(p, {}, {}, {});",
                vec3(*a),
                vec3(*b),
                number(*radius, id)?
            ),
            Node::Torus {
                center,
                normal,
                major,
                minor,
            } => format!(
                "return zs_grad_torus(p, {}, {}, {}, {});",
                vec3(*center),
                vec3(normalized_axis(*normal)),
                number(*major, id)?,
                number(*minor, id)?
            ),
            Node::Union { a, b } => {
                let fa = self.emit(*a)?;
                let fb = self.emit(*b)?;
                let ga = self.emit_grad(*a)?;
                let gb = self.emit_grad(*b)?;
                format!("return select({gb}(p), {ga}(p), {fa}(p) < {fb}(p));")
            }
            Node::Intersect { a, b } => {
                let fa = self.emit(*a)?;
                let fb = self.emit(*b)?;
                let ga = self.emit_grad(*a)?;
                let gb = self.emit_grad(*b)?;
                format!("return select({gb}(p), {ga}(p), {fa}(p) > {fb}(p));")
            }
            Node::Subtract { a, b } => {
                let fa = self.emit(*a)?;
                let fb = self.emit(*b)?;
                let ga = self.emit_grad(*a)?;
                let gb = self.emit_grad(*b)?;
                format!("return select(-{gb}(p), {ga}(p), {fa}(p) > -{fb}(p));")
            }
            Node::SmoothUnion { a, b, k } => {
                let fa = self.emit(*a)?;
                let fb = self.emit(*b)?;
                let ga = self.emit_grad(*a)?;
                let gb = self.emit_grad(*b)?;
                format!(
                    "return zs_smin_grad({fa}(p), {fb}(p), {ga}(p), {gb}(p), {});",
                    number(*k, id)?
                )
            }
            Node::SmoothIntersect { a, b, k } => {
                let fa = self.emit(*a)?;
                let fb = self.emit(*b)?;
                let ga = self.emit_grad(*a)?;
                let gb = self.emit_grad(*b)?;
                format!(
                    "return zs_smax_grad({fa}(p), {fb}(p), {ga}(p), {gb}(p), {});",
                    number(*k, id)?
                )
            }
            Node::SmoothSubtract { a, b, k } => {
                let fa = self.emit(*a)?;
                let fb = self.emit(*b)?;
                let ga = self.emit_grad(*a)?;
                let gb = self.emit_grad(*b)?;
                format!(
                    "return zs_ssub_grad({fa}(p), {fb}(p), {ga}(p), {gb}(p), {});",
                    number(*k, id)?
                )
            }
            Node::Offset { child, .. } => {
                let child = self.emit_grad(*child)?;
                format!("return {child}(p);")
            }
            Node::Transform { child, transform } => {
                let child = self.emit_grad(*child)?;
                let matrix = Mat3::from_quat(transform.rotation).transpose();
                let columns = matrix.to_cols_array();
                let mat = format!(
                    "mat3x3<f32>({},{},{},{},{},{},{},{},{})",
                    number(columns[0], id)?,
                    number(columns[1], id)?,
                    number(columns[2], id)?,
                    number(columns[3], id)?,
                    number(columns[4], id)?,
                    number(columns[5], id)?,
                    number(columns[6], id)?,
                    number(columns[7], id)?,
                    number(columns[8], id)?
                );
                let scale = number(transform.scale, id)?;
                format!(
                    "return transpose({mat}) * {child}({mat} * (p - {}) / {scale});",
                    vec3(transform.translation)
                )
            }
        };

        writeln!(
            self.grad_functions,
            "fn {name}(p: vec3<f32>) -> vec3<f32> {{ {body} }}"
        )
        .expect("writing to String cannot fail");
        self.grad_emitted.insert(id);
        Ok(name)
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

fn vec3(value: Vec3) -> String {
    format!(
        "vec3<f32>({},{},{})",
        float(value.x),
        float(value.y),
        float(value.z)
    )
}

fn float(value: f32) -> String {
    // Debug for f32 is shortest-roundtrip and always includes a fraction or
    // exponent, which keeps WGSL from treating it as an integer literal.
    format!("{value:?}")
}

fn number(value: f32, id: NodeId) -> Result<String, GpuError> {
    if value.is_finite() {
        Ok(float(value))
    } else {
        Err(GpuError::NonFinite(id.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeroset_core::{FieldGraph, Transform, Vec3};

    #[test]
    fn sphere_shader_contains_entry_point_and_field() {
        let mut graph = FieldGraph::new();
        let root = graph.sphere(Vec3::ZERO, 1.0);
        graph.set_root(root);
        let shader = compile_shader(&graph).unwrap();
        assert!(shader.contains("@compute @workgroup_size(64)"));
        assert!(shader.contains("fn zs_field"));
        assert!(shader.contains("fn zs_gradient"));
        assert!(shader.contains("fn zs_grad_"));
        assert!(shader.contains("length(p - vec3<f32>(0.0,0.0,0.0)) - 1.0"));
    }

    #[test]
    fn raymarch_shader_contains_render_entry_point() {
        let mut graph = FieldGraph::new();
        let root = graph.sphere(Vec3::ZERO, 1.0);
        graph.set_root(root);
        let shader = compile_raymarch_shader(&graph).unwrap();
        assert!(shader.contains("fn zs_render"));
        assert!(shader.contains("fn zs_field"));
        assert!(shader.contains("fn zs_gradient"));
        assert!(shader.contains("@workgroup_size(8, 8)"));
    }

    #[test]
    fn raymarch_texture_shader_contains_storage_texture() {
        let mut graph = FieldGraph::new();
        let root = graph.sphere(Vec3::ZERO, 1.0);
        graph.set_root(root);
        let shader = compile_raymarch_texture_shader(&graph).unwrap();
        assert!(shader.contains("texture_storage_2d<rgba16float, write>"));
        assert!(shader.contains("textureStore"));
        assert!(shader.contains("fn zs_shade"));
    }

    #[test]
    fn taa_shader_contains_reprojection() {
        let shader = compile_taa_shader();
        assert!(shader.contains("fn zs_taa"));
        assert!(shader.contains("uv_prev"));
        assert!(shader.contains("textureSampleLevel"));
        assert!(shader.contains("zs_prev"));
    }

    #[test]
    fn gpu_analytic_gradient_matches_cpu_bracket() {
        let graph = zeroset_core::presets::bracket();
        let functions = compile_field_functions(&graph).unwrap();
        let shader = format!(
            "{functions}
@group(0) @binding(0) var<storage, read> zs_positions: array<f32>;
@group(0) @binding(1) var<storage, read_write> zs_gradients: array<f32>;

@compute @workgroup_size(1)
fn zs_probe(@builtin(global_invocation_id) gid: vec3<u32>) {{
    let base = gid.x * 3u;
    let p = vec3<f32>(
        zs_positions[base],
        zs_positions[base + 1u],
        zs_positions[base + 2u]
    );
    let gradient = zs_gradient(p);
    zs_gradients[base] = gradient.x;
    zs_gradients[base + 1u] = gradient.y;
    zs_gradients[base + 2u] = gradient.z;
}}
"
        );

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter =
            match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
                ..Default::default()
            })) {
                Ok(adapter) => adapter,
                Err(error) => {
                    eprintln!("skipping GPU gradient test: {error}");
                    return;
                }
            };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("zeroset-gradient-probe"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .unwrap();

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("zeroset-gradient-probe"),
            source: wgpu::ShaderSource::Wgsl(shader.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("zeroset-gradient-probe"),
            layout: None,
            module: &module,
            entry_point: Some("zs_probe"),
            compilation_options: Default::default(),
            cache: None,
        });

        let points = [
            Vec3::ZERO,
            Vec3::new(20.0, 0.0, 0.0),
            Vec3::new(-20.0, 0.0, 0.0),
            Vec3::new(0.0, 20.0, 0.0),
            Vec3::new(0.0, 0.0, 15.0),
            Vec3::new(10.0, 5.0, 3.0),
            Vec3::new(-10.0, -5.0, -3.0),
            Vec3::new(30.0, 20.0, 10.0),
            Vec3::new(-30.0, -20.0, -10.0),
            Vec3::new(5.0, -15.0, 8.0),
        ];
        let mut flat = Vec::with_capacity(points.len() * 3);
        for point in &points {
            flat.extend_from_slice(&point.to_array());
        }
        let byte_len = (flat.len() * 4) as u64;

        let positions = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zs_gradient_positions"),
            size: byte_len,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&positions, 0, bytemuck::cast_slice(&flat));
        let gradients = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zs_gradient_values"),
            size: byte_len,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zs_gradient_staging"),
            size: byte_len,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let layout = pipeline.get_bind_group_layout(0);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("zs_gradient_bind_group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: positions.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: gradients.as_entire_binding(),
                },
            ],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("zs_gradient_encoder"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("zs_gradient_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(points.len() as u32, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&gradients, 0, &staging, 0, byte_len);
        queue.submit(Some(encoder.finish()));

        let slice = staging.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receiver.recv().unwrap().unwrap();

        let view = slice.get_mapped_range().unwrap();
        let mut gpu = Vec::with_capacity(flat.len());
        for chunk in view.chunks_exact(4) {
            gpu.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        drop(view);
        staging.unmap();

        for (index, point) in points.iter().enumerate() {
            let cpu = graph.gradient(*point);
            let actual = Vec3::new(gpu[index * 3], gpu[index * 3 + 1], gpu[index * 3 + 2]);
            let error = (actual - cpu).length();
            assert!(
                error < 1e-2,
                "gradient at {point:?}: gpu={actual:?} cpu={cpu:?} error={error}"
            );
        }
    }

    #[test]
    fn transform_uses_inverse_point() {
        let mut graph = FieldGraph::new();
        let sphere = graph.sphere(Vec3::ZERO, 1.0);
        let moved = graph.transform(
            sphere,
            Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)),
        );
        graph.set_root(moved);
        let shader = compile_shader(&graph).unwrap();
        assert!(shader.contains("p - vec3<f32>(3.0,0.0,0.0)"));
    }

    #[test]
    fn empty_graph_is_rejected() {
        let graph = FieldGraph::new();
        assert!(matches!(compile_shader(&graph), Err(GpuError::EmptyGraph)));
    }
}
