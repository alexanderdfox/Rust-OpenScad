//! openscad_rs library — parse/eval OpenSCAD subset → Manifold geometry

pub mod ast;
pub mod eval;
pub mod parser;
pub mod stl;

pub use anyhow::{Error, Result};
pub use manifold_rust::manifold::Manifold;

/// Parse OpenSCAD source and evaluate to a Manifold solid.
pub fn compile(source: &str) -> Result<Manifold> {
    let program = parser::parse(source)?;
    eval::evaluate(&program)
}
