//! CLI entry point

use anyhow::{bail, Context, Result};
use manifold_rust::linalg::Vec3;
use manifold_rust::manifold::Manifold;
use openscad_rs::stl;
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut output = PathBuf::from("out.stl");
    let mut demo: Option<String> = None;
    let mut input: Option<PathBuf> = None;

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
            "-h" | "--help" => {
                print_help();
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

    println!("openscad_rs — pure Rust OpenSCAD core (Manifold, no CGAL)");

    let model = if let Some(path) = input {
        println!("Parsing {}", path.display());
        let source =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        openscad_rs::compile(&source).context("evaluation error")?
    } else {
        let name = demo.unwrap_or_else(|| "tree".into());
        println!("Building demo: {}", name);
        match name.as_str() {
            "tree" => build_tree(),
            "cube" => Manifold::cube(Vec3::new(20.0, 20.0, 20.0), true),
            "difference" => {
                let cube = Manifold::cube(Vec3::new(20.0, 20.0, 20.0), true);
                let sphere = Manifold::sphere(12.0, 32);
                cube.difference(&sphere)
            }
            "cylinder" => Manifold::cylinder(30.0, 8.0, 8.0, 32),
            other => bail!(
                "Unknown demo '{}'. Use: tree | cube | difference | cylinder",
                other
            ),
        }
    };

    if model.status() != manifold_rust::types::Error::NoError {
        bail!("Geometry error: {:?}", model.status());
    }

    println!(
        "  volume = {:.3}  surface_area = {:.3}  triangles = {}",
        model.volume(),
        model.surface_area(),
        model.num_tri()
    );

    stl::write_binary_stl(&model, &output)
        .with_context(|| format!("writing {}", output.display()))?;
    println!("Wrote {}", output.display());
    Ok(())
}

fn print_help() {
    println!(
        "Usage: openscad_rs [FILE.scad] [--demo NAME] [-o FILE.stl]

Also: openscad_rs_gui  for the OpenSCAD-like editor GUI
"
    );
}

fn build_tree() -> Manifold {
    let trunk = Manifold::cylinder(30.0, 8.0, 8.0, 32);
    let foliage = Manifold::sphere(20.0, 32).translate(Vec3::new(0.0, 0.0, 40.0));
    trunk.union(&foliage)
}
