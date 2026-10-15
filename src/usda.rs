//! USDA (Universal Scene Description ASCII) mesh export with per-part displayColor.

use anyhow::Result;
use manifold_rust::manifold::Manifold;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::{Model, Rgba};

/// Write all colored parts of a model as separate Mesh prims under one Xform.
pub fn write_usda_model(model: &Model, path: &Path, prim_name: &str) -> Result<()> {
    let safe_root: String = prim_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    let safe_root = if safe_root.is_empty()
        || safe_root
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(true)
    {
        format!("Mesh_{}", safe_root)
    } else {
        safe_root
    };

    let f = File::create(path)?;
    let mut w = BufWriter::new(f);
    writeln!(w, "#usda 1.0")?;
    writeln!(w, "(")?;
    writeln!(w, "    defaultPrim = \"{}\"", safe_root)?;
    writeln!(w, "    upAxis = \"Z\"")?;
    writeln!(w, "    metersPerUnit = 0.001")?;
    writeln!(w, ")")?;
    writeln!(w)?;
    writeln!(w, "def Xform \"{}\" (", safe_root)?;
    writeln!(w, "    kind = \"component\"")?;
    writeln!(w, ")")?;
    writeln!(w, "{{")?;

    for (i, part) in model.parts.iter().enumerate() {
        if part.solid.num_tri() == 0 {
            continue;
        }
        let name = format!("part_{}", i);
        write_mesh_prim(&mut w, &part.solid, part.color, &name, 2)?;
    }

    writeln!(w, "}}")?;
    w.flush()?;
    Ok(())
}

/// Single-mesh convenience wrapper.
pub fn write_usda(
    m: &Manifold,
    path: &Path,
    color: Option<Rgba>,
    prim_name: &str,
) -> Result<()> {
    // Build a temporary one-part model without requiring Manifold: Clone
    // by writing directly.
    let safe_root: String = prim_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    let f = File::create(path)?;
    let mut w = BufWriter::new(f);
    writeln!(w, "#usda 1.0")?;
    writeln!(w, "(")?;
    writeln!(w, "    defaultPrim = \"{}\"", safe_root)?;
    writeln!(w, "    upAxis = \"Z\"")?;
    writeln!(w, ")")?;
    writeln!(w, "def Xform \"{}\" {{", safe_root)?;
    write_mesh_prim(&mut w, m, color, "geometry", 1)?;
    writeln!(w, "}}")?;
    w.flush()?;
    Ok(())
}

fn write_mesh_prim(
    w: &mut BufWriter<File>,
    m: &Manifold,
    color: Option<Rgba>,
    name: &str,
    indent: usize,
) -> Result<()> {
    let pad = " ".repeat(indent * 4);
    let mesh = m.get_mesh_gl(0);
    let num_prop = (mesh.num_prop as usize).max(3);
    let verts = &mesh.vert_properties;
    let tris = &mesh.tri_verts;
    let n_tri = tris.len() / 3;
    let n_vert = verts.len() / num_prop;
    let rgba = color.unwrap_or([0.82, 0.68, 0.37, 1.0]);

    writeln!(w, "{}def Mesh \"{}\" {{", pad, name)?;
    writeln!(w, "{}    uniform token subdivisionScheme = \"none\"", pad)?;

    write!(w, "{}    int[] faceVertexCounts = [", pad)?;
    for i in 0..n_tri {
        if i > 0 {
            write!(w, ", ")?;
        }
        if i % 32 == 0 {
            write!(w, "\n{}        ", pad)?;
        }
        write!(w, "3")?;
    }
    writeln!(w, "\n{}    ]", pad)?;

    write!(w, "{}    int[] faceVertexIndices = [", pad)?;
    for i in 0..n_tri {
        if i > 0 {
            write!(w, ", ")?;
        }
        if i % 8 == 0 {
            write!(w, "\n{}        ", pad)?;
        }
        write!(
            w,
            "{}, {}, {}",
            tris[i * 3],
            tris[i * 3 + 1],
            tris[i * 3 + 2]
        )?;
    }
    writeln!(w, "\n{}    ]", pad)?;

    write!(w, "{}    point3f[] points = [", pad)?;
    for i in 0..n_vert {
        if i > 0 {
            write!(w, ", ")?;
        }
        if i % 4 == 0 {
            write!(w, "\n{}        ", pad)?;
        }
        let base = i * num_prop;
        write!(
            w,
            "({}, {}, {})",
            verts[base],
            verts[base + 1],
            verts[base + 2]
        )?;
    }
    writeln!(w, "\n{}    ]", pad)?;

    writeln!(
        w,
        "{}    color3f[] primvars:displayColor = [({}, {}, {})] (",
        pad, rgba[0], rgba[1], rgba[2]
    )?;
    writeln!(w, "{}        interpolation = \"constant\"", pad)?;
    writeln!(w, "{}    )", pad)?;
    if rgba[3] < 0.999 {
        writeln!(
            w,
            "{}    float[] primvars:displayOpacity = [{}] (",
            pad, rgba[3]
        )?;
        writeln!(w, "{}        interpolation = \"constant\"", pad)?;
        writeln!(w, "{}    )", pad)?;
    }
    writeln!(w, "{}}}", pad)?;
    Ok(())
}
