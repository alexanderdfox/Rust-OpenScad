//! CLI entry point for openscad_rs

use anyhow::{bail, Context, Result};
use manifold_rust::linalg::Vec3;
use manifold_rust::manifold::Manifold;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut output = PathBuf::from("out.stl");
    let mut demo: Option<String> = None;
    let mut input: Option<PathBuf> = None;
    let mut quiet = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    bail!("-o requires a path");
                }
                output = PathBuf::from(&args[i]);
            }
            "-d" | "--demo" => {
                i += 1;
                if i >= args.len() {
                    bail!("--demo requires a name");
                }
                demo = Some(args[i].clone());
            }
            "-q" | "--quiet" => quiet = true,
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            "-V" | "--version" => {
                println!("openscad_rs {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            other if other.starts_with('-') => bail!("unknown flag '{}'", other),
            other => {
                if other.ends_with(".scad") {
                    input = Some(PathBuf::from(other));
                } else if demo.is_none() {
                    demo = Some(other.to_string());
                }
            }
        }
        i += 1;
    }

    if !quiet {
        println!("openscad_rs v{} — pure Rust OpenSCAD core (Manifold, no CGAL)", env!("CARGO_PKG_VERSION"));
    }

    let t0 = Instant::now();
    let model = if let Some(path) = input {
        if !quiet {
            println!("Parsing {}", path.display());
        }
        let source =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        openscad_rs::compile(&source).context("evaluation error")?
    } else {
        let name = demo.unwrap_or_else(|| "tree".into());
        if !quiet {
            println!("Building demo: {}", name);
        }
        let solid = match name.as_str() {
            "tree" => build_tree(),
            "cube" => Manifold::cube(Vec3::new(20.0, 20.0, 20.0), true),
            "difference" => {
                let cube = Manifold::cube(Vec3::new(20.0, 20.0, 20.0), true);
                let sphere = Manifold::sphere(12.0, 32);
                cube.difference(&sphere)
            }
            "cylinder" => Manifold::cylinder(30.0, 8.0, 8.0, 32),
            "box" => {
                let box_ = Manifold::cube(Vec3::new(30.0, 20.0, 10.0), true);
                let hole1 = Manifold::cylinder(20.0, 3.0, 3.0, 24).translate(Vec3::new(8.0, 0.0, 0.0));
                let hole2 = Manifold::cylinder(20.0, 3.0, 3.0, 24).translate(Vec3::new(-8.0, 0.0, 0.0));
                box_.difference(&hole1).difference(&hole2)
            }
            other => bail!(
                "Unknown demo '{}'. Available: tree | cube | difference | cylinder | box",
                other
            ),
        };
        openscad_rs::Model {
            parts: vec![openscad_rs::ColoredPart {
                solid,
                color: None,
            }],
        }
    };

    if model.status() != manifold_rust::types::Error::NoError {
        bail!("Geometry error: {:?}", model.status());
    }

    let elapsed = t0.elapsed();
    if !quiet {
        println!(
            "  volume = {:.3}  surface_area = {:.3}  triangles = {}  ({:.0} ms)",
            model.volume(),
            model.surface_area(),
            model.num_tri(),
            elapsed.as_secs_f64() * 1000.0
        );
    }

    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("stl")
        .to_ascii_lowercase();
    match ext.as_str() {
        "usda" | "usd" => {
            let name = output
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Model");
            openscad_rs::usda::write_usda_model(&model, &output, name)
                .with_context(|| format!("writing {}", output.display()))?;
            if !quiet {
                for (i, p) in model.parts.iter().enumerate() {
                    if let Some(c) = p.color {
                        println!(
                            "  part[{}] color = rgba({:.3}, {:.3}, {:.3}, {:.3})",
                            i, c[0], c[1], c[2], c[3]
                        );
                    }
                }
            }
        }
        _ => {
            openscad_rs::stl::write_binary_stl(&model.solid(), &output)
                .with_context(|| format!("writing {}", output.display()))?;
        }
    }
    if !quiet {
        println!("Wrote {}", output.display());
    }
    Ok(())
}

fn print_help() {
    println!(
        "openscad_rs — pure-Rust OpenSCAD subset (Manifold backend)

USAGE:
    openscad_rs [FILE.scad] [OPTIONS]
    openscad_rs --demo NAME [OPTIONS]

OPTIONS:
    -o, --output <FILE>   Output STL path (default: out.stl)
    -d, --demo <NAME>     Built-in demo: tree | cube | difference | cylinder | box
    -q, --quiet           Minimal output
    -h, --help            Show this help
    -V, --version         Print version

EXAMPLES:
    openscad_rs model.scad -o model.stl
    openscad_rs --demo tree -o tree.stl
    openscad_rs_gui                       # Launch the interactive editor

OUTPUT formats (by extension):
    .stl   binary STL (default)
    .usda  USD ASCII mesh with displayColor

Also see: cargo run --release --bin openscad_rs_gui
"
    );
}

fn build_tree() -> Manifold {
    let trunk = Manifold::cylinder(30.0, 8.0, 8.0, 32);
    let foliage = Manifold::sphere(20.0, 32).translate(Vec3::new(0.0, 0.0, 40.0));
    trunk.union(&foliage)
}
