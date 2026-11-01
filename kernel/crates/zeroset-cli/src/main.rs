use std::error::Error;
use std::fs;
use std::time::Instant;

use zeroset_core::{Aabb, FieldGraph, Vec3, category_name, presets, stl};
use zeroset_gpu::{
    AdaptiveConfig, GpuSampler, mesh_adaptive_mixed, mesh_adaptive_mixed_dc, mesh_adaptive_refined,
    sample_adaptive,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("mesh") => cmd_mesh(&args[1..]),
        Some("adaptive") => cmd_adaptive(&args[1..]),
        Some("sample") => cmd_sample(&args[1..]),
        Some("info") => cmd_info(&args[1..]),
        Some("ir") => cmd_ir(&args[1..]),
        Some("presets") => {
            for name in presets::PRESETS {
                println!("{name}");
            }
            Ok(())
        }
        Some("--help" | "-h" | "help") | None => {
            print_usage();
            Ok(())
        }
        Some(other) => Err(bad_input(format!("unknown command `{other}`"))),
    }
}

fn cmd_mesh(args: &[String]) -> Result<(), Box<dyn Error>> {
    let preset = value(args, "--preset").unwrap_or_else(|| "bracket".to_string());
    let resolution = value(args, "--res")
        .map(|value| value.parse::<u32>())
        .transpose()?
        .unwrap_or(64);
    let method = value(args, "--method").unwrap_or_else(|| "surface".to_string());
    let precision = value(args, "--precision").unwrap_or_else(|| "f32".to_string());
    let compare = args.iter().any(|arg| arg == "--compare");
    let hausdorff_bounds = args.iter().any(|arg| arg == "--hausdorff-bounds");
    let hausdorff_tolerance = value(args, "--hausdorff-tolerance")
        .map(|value| value.parse::<f32>())
        .transpose()?;
    let crease = value(args, "--crease")
        .map(|value| value.parse::<f32>())
        .transpose()?;
    let simplify = value(args, "--simplify")
        .map(|value| value.parse::<f32>())
        .transpose()?;
    let simplify_method =
        value(args, "--simplify-method").unwrap_or_else(|| "collapse".to_string());
    let simplify_angle = value(args, "--simplify-angle")
        .map(|value| value.parse::<f32>())
        .transpose()?
        .unwrap_or(30.0);
    let feature_angle = value(args, "--feature-angle")
        .map(|value| value.parse::<f32>())
        .transpose()?
        .unwrap_or(30.0);
    let feature_relax = value(args, "--feature-relax")
        .map(|value| value.parse::<u32>())
        .transpose()?
        .unwrap_or(0);
    let show_features = args.iter().any(|arg| arg == "--features");
    let output = value(args, "--out")
        .unwrap_or_else(|| format!("{preset}-{resolution}-{method}-{precision}.stl"));

    let graph = load_preset(&preset)?;

    let started = Instant::now();
    let (mesh, sample_time, mesh_time) = if precision == "f64" {
        let grid = graph.sample_grid_f64(resolution)?;
        let sample_time = started.elapsed();
        let started = Instant::now();
        let mesh = match method.as_str() {
            "dc" | "dual_contour" => {
                let mesh =
                    grid.dual_contour_world_with_gradient(|point| graph.gradient_f64(point))?;
                mesh.to_f32_world()
            }
            _ => grid.surface_nets_world()?.to_f32_world(),
        };
        (mesh, sample_time, started.elapsed())
    } else {
        let grid = graph.sample_grid(resolution)?;
        let sample_time = started.elapsed();
        let started = Instant::now();
        let mesh = match method.as_str() {
            "dc" | "dual_contour" => graph.dual_contour(resolution)?,
            _ => grid.surface_nets()?,
        };
        (mesh, sample_time, started.elapsed())
    };

    let mesh = if feature_relax > 0 {
        mesh.relax_feature_chains(feature_angle, feature_relax)
    } else {
        mesh
    };
    let original_triangles = mesh.triangle_count();
    let original_mesh = mesh.clone();
    let mesh = if let Some(ratio) = simplify {
        match simplify_method.as_str() {
            "cluster" | "clustered" => mesh.simplify_clustered(ratio),
            _ => mesh.simplify_edge_collapse_with_angle(ratio, simplify_angle),
        }
    } else {
        mesh
    };
    stl::write_stl_binary_file(&output, &mesh)?;
    let stats = mesh.stats();
    if feature_relax > 0 {
        println!("relax       : {feature_relax} iterations at {feature_angle} deg");
    }
    if show_features {
        let edges = mesh.feature_edges(feature_angle);
        let chains = mesh.feature_chains(feature_angle);
        println!(
            "features    : {} edges, {} chains ({feature_angle} deg)",
            edges.len(),
            chains.len()
        );
    }
    if let Some(ratio) = simplify {
        let hausdorff = original_mesh.hausdorff_distance_tolerance(&mesh, 0.05);
        println!(
            "simplify    : {} ratio {ratio}, angle {simplify_angle}, triangles {original_triangles} -> {}, hausdorff {hausdorff:.6}",
            simplify_method,
            mesh.triangle_count()
        );
    }
    if let Some(degrees) = crease {
        let split = mesh.with_crease_normals(degrees);
        println!(
            "crease      : {degrees} deg, split vertices {} -> {}",
            mesh.vertex_count(),
            split.vertex_count()
        );
    }
    let (hausdorff, hausdorff_interval) = if compare {
        let baseline = graph.sample_grid(resolution)?.surface_nets()?;
        if hausdorff_bounds {
            let bounds = baseline.hausdorff_bounds(&mesh, hausdorff_tolerance.unwrap_or(0.05));
            (None, Some(bounds))
        } else {
            let value = match hausdorff_tolerance {
                Some(tolerance) => baseline.hausdorff_distance_tolerance(&mesh, tolerance),
                None => baseline.hausdorff_distance(&mesh, 3),
            };
            (Some(value), None)
        }
    } else {
        (None, None)
    };

    println!("preset      : {preset}");
    println!("resolution  : {resolution}");
    println!("method      : {method}");
    println!("precision   : {precision}");
    println!("samples     : ({sample_time:?})");
    println!(
        "mesh        : {} vertices, {} triangles ({mesh_time:?})",
        stats.vertices, stats.triangles
    );
    println!(
        "topology    : closed={} boundary_edges={} non_manifold_edges={}",
        stats.is_closed, stats.boundary_edges, stats.non_manifold_edges
    );
    println!("volume      : {:.6}", stats.signed_volume);
    if let Some(bounds) = hausdorff_interval {
        println!(
            "hausdorff   : [{:.6}, {:.6}] (global interval)",
            bounds.lower, bounds.upper
        );
    }
    if let Some(distance) = hausdorff {
        match hausdorff_tolerance {
            Some(tolerance) => println!("hausdorff   : {distance:.6} (tolerance {tolerance})"),
            None => println!("hausdorff   : {distance:.6} (vs f32 Surface Nets)"),
        }
    }
    println!("output      : {output}");
    Ok(())
}

fn cmd_sample(args: &[String]) -> Result<(), Box<dyn Error>> {
    let preset = value(args, "--preset").unwrap_or_else(|| "sphere".to_string());
    let graph = load_preset(&preset)?;

    let points = values(args, "--point");
    if points.is_empty() {
        return Err(bad_input("sample needs at least one --point x,y,z"));
    }

    for text in points {
        let point = parse_point(&text)?;
        let value = graph.evaluate(point);
        let gradient = graph.gradient(point);
        println!(
            "{:?} -> value={value:.6} gradient_norm={:.6}",
            point.to_array(),
            gradient.length()
        );
    }
    Ok(())
}

fn cmd_adaptive(args: &[String]) -> Result<(), Box<dyn Error>> {
    let preset = value(args, "--preset").unwrap_or_else(|| "sphere".to_string());
    let base = value(args, "--base")
        .map(|value| value.parse::<u32>())
        .transpose()?
        .unwrap_or(16);
    let depth = value(args, "--depth")
        .map(|value| value.parse::<u32>())
        .transpose()?
        .unwrap_or(2);
    let tiles = value(args, "--tiles")
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(24);
    let band = value(args, "--band")
        .map(|value| value.parse::<f32>())
        .transpose()?
        .unwrap_or(2.0);
    let method = value(args, "--method").unwrap_or_else(|| "mixed-dc".to_string());
    let compare = args.iter().any(|arg| arg == "--compare");
    let hausdorff_tolerance = value(args, "--hausdorff-tolerance")
        .map(|value| value.parse::<f32>())
        .transpose()?;
    let hausdorff_bounds = args.iter().any(|arg| arg == "--hausdorff-bounds");
    let cap = args.iter().any(|arg| arg == "--cap");
    let simplify = value(args, "--simplify")
        .map(|value| value.parse::<f32>())
        .transpose()?
        .unwrap_or(1.0);
    let simplify_angle = value(args, "--simplify-angle")
        .map(|value| value.parse::<f32>())
        .transpose()?
        .unwrap_or(30.0);
    let output = value(args, "--out").unwrap_or_else(|| format!("{preset}-adaptive-{method}.stl"));

    let graph = load_preset(&preset)?;
    let bounds = match (value(args, "--bounds-min"), value(args, "--bounds-max")) {
        (Some(min), Some(max)) => Aabb::new(parse_point(&min)?, parse_point(&max)?),
        (None, None) => graph.bounds(),
        _ => {
            return Err(bad_input(
                "--bounds-min and --bounds-max must be provided together".to_string(),
            ));
        }
    };
    let sampler = GpuSampler::new()?;
    let config = AdaptiveConfig {
        bounds,
        base_cells: base,
        max_depth: depth,
        band_cells: band,
        max_tiles: tiles,
    };
    let started = Instant::now();
    let adaptive = sample_adaptive(&sampler, &graph, &config)?;
    let mut mesh = match method.as_str() {
        "mixed" | "sn" => mesh_adaptive_mixed(&graph, &adaptive)?,
        "mixed-dc" | "dc" => mesh_adaptive_mixed_dc(&graph, &adaptive)?,
        other => {
            return Err(bad_input(format!(
                "unknown adaptive method `{other}`; use mixed or mixed-dc"
            )));
        }
    };
    if simplify < 0.999 {
        mesh = mesh.simplify_edge_collapse_with_angle(simplify, simplify_angle);
    }
    let mut cap_report = None;
    if cap {
        let (capped, report) = mesh.cap_boundary_loops_with_report();
        mesh = capped;
        cap_report = Some(report);
    }
    let stats = mesh.stats();
    let (hausdorff, hausdorff_bounds_value) = if compare {
        let refined = mesh_adaptive_refined(&graph, &adaptive)?;
        let distance = match hausdorff_tolerance {
            Some(tolerance) => refined.hausdorff_distance_tolerance(&mesh, tolerance),
            None => refined.hausdorff_distance(&mesh, 3),
        };
        let bounds = hausdorff_bounds
            .then(|| refined.hausdorff_bounds(&mesh, hausdorff_tolerance.unwrap_or(0.05)));
        (Some(distance), bounds)
    } else {
        (None, None)
    };
    let mut bytes = Vec::new();
    stl::write_stl_binary(&mesh, &mut bytes)?;
    fs::write(&output, &bytes)?;

    println!("preset      : {preset}");
    println!("backend     : {}", sampler.adapter_name());
    println!("method      : {method}");
    match cap_report {
        Some(report) => println!(
            "cap         : true (loops={} skipped_edges={} nonmanifold_edges={} branch_vertices={})",
            report.loops, report.skipped_edges, report.non_manifold_edges, report.branch_vertices
        ),
        None => println!("cap         : false"),
    }
    println!("simplify    : {simplify} (angle {simplify_angle})");
    println!(
        "tiles       : {} (truncated={})",
        adaptive.tiles.len(),
        adaptive.truncated
    );
    println!("points      : {}", adaptive.point_count());
    println!(
        "mesh        : {} vertices, {} triangles",
        stats.vertices, stats.triangles
    );
    println!(
        "topology    : closed={} boundary_edges={} non_manifold_edges={}",
        stats.is_closed, stats.boundary_edges, stats.non_manifold_edges
    );
    println!("volume      : {:.6}", stats.signed_volume);
    if let Some(hausdorff) = hausdorff {
        println!("hausdorff   : {hausdorff:.6} (vs refined)");
    }
    if let Some(bounds) = hausdorff_bounds_value {
        println!(
            "hausdorff b : lower={:.6} upper={:.6}",
            bounds.lower, bounds.upper
        );
    }
    println!("time        : {:?}", started.elapsed());
    println!("wrote       : {output} ({} bytes)", bytes.len());
    Ok(())
}

fn cmd_info(args: &[String]) -> Result<(), Box<dyn Error>> {
    let preset = value(args, "--preset").unwrap_or_else(|| "bracket".to_string());
    let graph = load_preset(&preset)?;
    let bounds = graph.bounds();

    println!("preset      : {preset}");
    println!("nodes       : {}", graph.len());
    println!("category    : {}", category_name(graph.category()));
    println!("bounds min  : {:?}", bounds.min.to_array());
    println!("bounds max  : {:?}", bounds.max.to_array());
    println!("bounds size : {:?}", bounds.size().to_array());
    Ok(())
}

fn cmd_ir(args: &[String]) -> Result<(), Box<dyn Error>> {
    let preset = value(args, "--preset").unwrap_or_else(|| "bracket".to_string());
    let graph = load_preset(&preset)?;
    println!("{}", serde_json::to_string_pretty(&graph)?);
    Ok(())
}

fn load_preset(name: &str) -> Result<FieldGraph, Box<dyn Error>> {
    let graph = presets::by_name(name).ok_or_else(|| {
        bad_input(format!(
            "unknown preset `{name}`; available: {}",
            presets::PRESETS.join(", ")
        ))
    })?;
    graph.validate()?;
    Ok(graph)
}

fn value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn values(args: &[String], name: &str) -> Vec<String> {
    let mut collected = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] == name {
            if let Some(value) = args.get(index + 1) {
                collected.push(value.clone());
                index += 2;
                continue;
            }
        }
        index += 1;
    }
    collected
}

fn parse_point(text: &str) -> Result<Vec3, Box<dyn Error>> {
    let components: Vec<f32> = text
        .split(',')
        .map(|component| component.trim().parse::<f32>())
        .collect::<Result<_, _>>()?;
    if components.len() != 3 {
        return Err(bad_input(format!(
            "point `{text}` must have three comma-separated numbers"
        )));
    }
    Ok(Vec3::new(components[0], components[1], components[2]))
}

fn bad_input(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message.into(),
    ))
}

fn print_usage() {
    println!(
        "\
ZeroSet kernel CLI

USAGE:
    zs mesh   [--preset <name>] [--res <n>] [--method surface|dc] [--precision f32|f64] [--compare] [--hausdorff-bounds] [--hausdorff-tolerance <x>] [--crease <deg>] [--simplify <ratio>] [--simplify-method collapse|cluster] [--simplify-angle <deg>] [--features] [--feature-angle <deg>] [--feature-relax <n>] [--out <file>]
    zs adaptive [--preset <name>] [--base <n>] [--depth <n>] [--tiles <n>] [--band <x>] [--bounds-min x,y,z --bounds-max x,y,z] [--cap] [--simplify <ratio>] [--simplify-angle <deg>] [--method mixed|mixed-dc] [--compare] [--hausdorff-tolerance <x>] [--hausdorff-bounds] [--out <file>]
    zs sample [--preset <name>] --point <x,y,z> [--point <x,y,z> ...]
    zs info   [--preset <name>]
    zs ir     [--preset <name>]
    zs presets

PRESETS:
    {}
",
        presets::PRESETS.join(", ")
    );
}
