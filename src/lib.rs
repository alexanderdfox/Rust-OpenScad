//! openscad_rs — pure-Rust OpenSCAD-like modeller (Manifold backend, no CGAL)

pub mod ast;
pub mod eval;
pub mod parser;
pub mod stl;
pub mod usda;

pub use anyhow::{Error, Result};
pub use manifold_rust::manifold::Manifold;

/// RGBA in 0..1
pub type Rgba = [f32; 4];

/// One colored solid piece (from a `color()` subtree or uncolored geometry).
pub struct ColoredPart {
    pub solid: Manifold,
    pub color: Option<Rgba>,
}

/// Compiled model: one or more colored parts.
pub struct Model {
    pub parts: Vec<ColoredPart>,
}

impl Model {
    pub fn solid(&self) -> Manifold {
        if self.parts.is_empty() {
            return Manifold::empty();
        }
        // Union without requiring Clone on every intermediate if possible:
        // start from first by re-importing mesh (works even if Manifold is not Clone).
        let mut acc = {
            let mesh = self.parts[0].solid.get_mesh_gl(0);
            Manifold::from_mesh_gl_robust(&mesh)
        };
        for p in self.parts.iter().skip(1) {
            let mesh = p.solid.get_mesh_gl(0);
            let other = Manifold::from_mesh_gl_robust(&mesh);
            acc = acc.union(&other);
        }
        acc
    }

    pub fn status(&self) -> manifold_rust::types::Error {
        for p in &self.parts {
            let s = p.solid.status();
            if s != manifold_rust::types::Error::NoError {
                return s;
            }
        }
        manifold_rust::types::Error::NoError
    }

    pub fn volume(&self) -> f64 {
        self.parts.iter().map(|p| p.solid.volume()).sum()
    }

    pub fn surface_area(&self) -> f64 {
        self.parts.iter().map(|p| p.solid.surface_area()).sum()
    }

    pub fn num_tri(&self) -> usize {
        self.parts.iter().map(|p| p.solid.num_tri() as usize).sum()
    }

    /// Primary / first color if any part has one.
    pub fn primary_color(&self) -> Option<Rgba> {
        self.parts.iter().find_map(|p| p.color)
    }
}

/// Parse OpenSCAD source and evaluate to a Model.
pub fn compile(source: &str) -> Result<Model> {
    let program = parser::parse(source)?;
    eval::evaluate(&program)
}
