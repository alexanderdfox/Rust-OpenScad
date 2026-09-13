//! Binary STL export from Manifold.

use anyhow::Result;
use manifold_rust::manifold::Manifold;
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub fn write_binary_stl(m: &Manifold, path: &Path) -> Result<()> {
    let mesh = m.get_mesh_gl(0);

    let num_prop = mesh.num_prop as usize;
    let verts = &mesh.vert_properties;
    let tris = &mesh.tri_verts;
    let n_tri = tris.len() / 3;

    let mut f = File::create(path)?;
    let header = b"openscad_rs (Manifold) binary STL";
    let mut hdr = [0u8; 80];
    let n = header.len().min(80);
    hdr[..n].copy_from_slice(&header[..n]);
    f.write_all(&hdr)?;
    f.write_all(&(n_tri as u32).to_le_bytes())?;

    for i in 0..n_tri {
        let i0 = tris[i * 3] as usize;
        let i1 = tris[i * 3 + 1] as usize;
        let i2 = tris[i * 3 + 2] as usize;

        let p0 = [
            verts[i0 * num_prop] as f32,
            verts[i0 * num_prop + 1] as f32,
            verts[i0 * num_prop + 2] as f32,
        ];
        let p1 = [
            verts[i1 * num_prop] as f32,
            verts[i1 * num_prop + 1] as f32,
            verts[i1 * num_prop + 2] as f32,
        ];
        let p2 = [
            verts[i2 * num_prop] as f32,
            verts[i2 * num_prop + 1] as f32,
            verts[i2 * num_prop + 2] as f32,
        ];

        let ux = p1[0] - p0[0];
        let uy = p1[1] - p0[1];
        let uz = p1[2] - p0[2];
        let vx = p2[0] - p0[0];
        let vy = p2[1] - p0[1];
        let vz = p2[2] - p0[2];
        let nx = uy * vz - uz * vy;
        let ny = uz * vx - ux * vz;
        let nz = ux * vy - uy * vx;
        let len = (nx * nx + ny * ny + nz * nz).sqrt();
        let (nx, ny, nz) = if len > 1e-12 {
            (nx / len, ny / len, nz / len)
        } else {
            (0.0, 0.0, 0.0)
        };

        for v in [nx, ny, nz] {
            f.write_all(&v.to_le_bytes())?;
        }
        for p in [&p0, &p1, &p2] {
            for c in p {
                f.write_all(&c.to_le_bytes())?;
            }
        }
        f.write_all(&0u16.to_le_bytes())?;
    }

    Ok(())
}
