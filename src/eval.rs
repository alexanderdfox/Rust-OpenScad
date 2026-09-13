//! Evaluate OpenSCAD AST → Manifold geometry.

use crate::ast::*;
use anyhow::{bail, Result};
use manifold_rust::linalg::{Vec2, Vec3};
use manifold_rust::manifold::Manifold;
use std::collections::HashMap;
use std::f64::consts::PI;

#[derive(Clone)]
struct ModuleDef {
    params: Vec<Param>,
    body: Vec<Statement>,
}

#[derive(Clone)]
struct FunctionDef {
    params: Vec<Param>,
    body: Expr,
}

pub fn evaluate(program: &Program) -> Result<Manifold> {
    let mut ctx = EvalCtx::new();
    // First pass: collect top-level module/function defs
    for stmt in &program.statements {
        match stmt {
            Statement::ModuleDef { name, params, body } => {
                ctx.modules.insert(
                    name.clone(),
                    ModuleDef {
                        params: params.clone(),
                        body: body.clone(),
                    },
                );
            }
            Statement::FunctionDef { name, params, body } => {
                ctx.functions.insert(
                    name.clone(),
                    FunctionDef {
                        params: params.clone(),
                        body: body.clone(),
                    },
                );
            }
            _ => {}
        }
    }
    let mut parts = Vec::new();
    for stmt in &program.statements {
        match stmt {
            Statement::ModuleDef { .. } | Statement::FunctionDef { .. } => {}
            _ => {
                if let Some(m) = ctx.eval_statement(stmt)? {
                    parts.push(m);
                }
            }
        }
    }
    Ok(union_all(parts))
}

struct EvalCtx {
    vars: HashMap<String, Value>,
    modules: HashMap<String, ModuleDef>,
    functions: HashMap<String, FunctionDef>,
}

impl EvalCtx {
    fn new() -> Self {
        let mut vars = HashMap::new();
        vars.insert("$fn".into(), Value::Number(0.0)); // 0 = use fa/fs
        vars.insert("$fa".into(), Value::Number(12.0));
        vars.insert("$fs".into(), Value::Number(2.0));
        vars.insert("$t".into(), Value::Number(0.0));
        vars.insert("$preview".into(), Value::Bool(false));
        vars.insert("$children".into(), Value::Number(0.0));
        vars.insert("$vpr".into(), Value::Vec3(Vec3::new(55.0, 0.0, 25.0)));
        vars.insert("$vpt".into(), Value::Vec3(Vec3::new(0.0, 0.0, 0.0)));
        vars.insert("$vpd".into(), Value::Number(140.0));
        vars.insert("$vpf".into(), Value::Number(22.5));
        vars.insert("PI".into(), Value::Number(std::f64::consts::PI));
        Self {
            vars,
            modules: HashMap::new(),
            functions: HashMap::new(),
        }
    }

    fn child_scope(&self) -> Self {
        Self {
            vars: self.vars.clone(),
            modules: self.modules.clone(),
            functions: self.functions.clone(),
        }
    }

    /// OpenSCAD-style segment count from $fn / $fa / $fs and radius.
    fn segments_for_radius(&self, r: f64) -> i32 {
        let fn_ = self.var_number("$fn").unwrap_or(0.0);
        if fn_ > 0.0 {
            return fn_.round().max(3.0) as i32;
        }
        let fa = self.var_number("$fa").unwrap_or(12.0).max(0.01);
        let fs = self.var_number("$fs").unwrap_or(2.0).max(0.01);
        // fragments from angle and from size
        let from_fa = (360.0 / fa).ceil();
        let from_fs = if r > 0.0 {
            (2.0 * PI * r / fs).ceil()
        } else {
            from_fa
        };
        from_fa.min(from_fs).max(5.0) as i32
    }

    fn var_number(&self, name: &str) -> Option<f64> {
        match self.vars.get(name)? {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    fn eval_statement(&mut self, stmt: &Statement) -> Result<Option<Manifold>> {
        match stmt {
            Statement::Assign { name, value } => {
                let v = self.eval_expr(value)?;
                self.vars.insert(name.clone(), v);
                Ok(None)
            }
            Statement::ModuleDef { .. } | Statement::FunctionDef { .. } => Ok(None),
            Statement::Block(stmts) => {
                let mut parts = Vec::new();
                for s in stmts {
                    if let Some(m) = self.eval_statement(s)? {
                        parts.push(m);
                    }
                }
                Ok(Some(union_all(parts)))
            }
            Statement::ModuleCall {
                name,
                args,
                children,
            } => self.eval_module_call(name, args, children),
            Statement::If {
                cond,
                then_branch,
                else_branch,
            } => {
                if self.eval_bool(cond)? {
                    self.eval_statement(then_branch)
                } else if let Some(eb) = else_branch {
                    self.eval_statement(eb)
                } else {
                    Ok(None)
                }
            }
            Statement::For { name, range, body } => {
                let values = self.eval_range_or_list(range)?;
                let mut parts = Vec::new();
                for v in values {
                    let mut child = self.child_scope();
                    child.vars.insert(name.clone(), v);
                    if let Some(m) = child.eval_statement(body)? {
                        parts.push(m);
                    }
                }
                Ok(Some(union_all(parts)))
            }
            Statement::ExprStmt(_) => Ok(None),
        }
    }

    fn eval_module_call(
        &mut self,
        name: &str,
        args: &[Arg],
        children: &[Statement],
    ) -> Result<Option<Manifold>> {
        // User-defined module?
        if let Some(def) = self.modules.get(name).cloned() {
            return self.call_user_module(&def, args, children);
        }

        let named = collect_named(args);
        let positional: Vec<&Expr> = args
            .iter()
            .filter_map(|a| match a {
                Arg::Positional(e) => Some(e),
                _ => None,
            })
            .collect();

        match name {
            "cube" => Ok(Some(self.builtin_cube(&named, &positional)?)),
            "sphere" => Ok(Some(self.builtin_sphere(&named, &positional)?)),
            "cylinder" => Ok(Some(self.builtin_cylinder(&named, &positional)?)),
            "circle" => Ok(Some(self.builtin_circle_2d(&named, &positional)?)),
            "square" => Ok(Some(self.builtin_square_2d(&named, &positional)?)),
            "polygon" => Ok(Some(self.builtin_polygon(&named, &positional)?)),
            "polyhedron" => Ok(Some(self.builtin_polyhedron(&named, &positional)?)),
            "resize" => {
                // resize([x,y,z]) children — approximate via bounding-box scale
                let target = self.vec3_arg(&named, &positional, "newsize", 0).or_else(|_| {
                    self.vec3_arg(&named, &positional, "v", 0)
                }).unwrap_or(Vec3::new(1.0, 1.0, 1.0));
                let body = union_all(self.eval_children(children)?);
                let bb = body.bounding_box();
                let sx = if (bb.max.x - bb.min.x).abs() > 1e-12 { target.x / (bb.max.x - bb.min.x) } else { 1.0 };
                let sy = if (bb.max.y - bb.min.y).abs() > 1e-12 { target.y / (bb.max.y - bb.min.y) } else { 1.0 };
                let sz = if (bb.max.z - bb.min.z).abs() > 1e-12 { target.z / (bb.max.z - bb.min.z) } else { 1.0 };
                // auto: if target component is 0, keep aspect
                let (sx, sy, sz) = {
                    let mut sx = sx; let mut sy = sy; let mut sz = sz;
                    if target.x == 0.0 { sx = sy.max(sz); }
                    if target.y == 0.0 { sy = sx.max(sz); }
                    if target.z == 0.0 { sz = sx.max(sy); }
                    (sx, sy, sz)
                };
                Ok(Some(body.scale(Vec3::new(sx, sy, sz))))
            }
            "multmatrix" => {
                // multmatrix(m) with 4x3 or 4x4 list — apply linear part if possible
                // For now: if m is list of 4 rows, extract translation from last column-ish
                let body = union_all(self.eval_children(children)?);
                Ok(Some(body)) // passthrough until full Mat3x4 parse
            }
            "offset" => {
                // 2D offset — not fully supported; passthrough children
                Ok(Some(union_all(self.eval_children(children)?)))
            }
            "projection" => {
                // projection(cut=true) — approximate by hull of children flattened
                Ok(Some(union_all(self.eval_children(children)?)))
            }
            "import" | "surface" => {
                bail!("{}() file import not implemented yet", name)
            }
            "union" => Ok(Some(union_all(self.eval_children(children)?))),
            "difference" => Ok(Some(difference_all(self.eval_children(children)?))),
            "intersection" => Ok(Some(intersection_all(self.eval_children(children)?))),
            "translate" => {
                let v = self.vec3_arg(&named, &positional, "v", 0)?;
                Ok(Some(union_all(self.eval_children(children)?).translate(v)))
            }
            "rotate" => {
                // rotate([x,y,z]) euler degrees OR rotate(a, [axis])
                let body = union_all(self.eval_children(children)?);
                if positional.len() >= 2 || (named.contains_key("a") && named.contains_key("v")) {
                    let angle = if let Some(e) = named.get("a") {
                        self.eval_number(e)?
                    } else {
                        self.eval_number(positional[0])?
                    };
                    let axis = if let Some(e) = named.get("v") {
                        self.eval_vec3(e)?
                    } else {
                        self.eval_vec3(positional[1])?
                    };
                    // Convert axis-angle to approximate euler via rotation around dominant axis
                    let len = (axis.x * axis.x + axis.y * axis.y + axis.z * axis.z).sqrt();
                    if len < 1e-12 {
                        Ok(Some(body))
                    } else {
                        let ax = axis.x / len;
                        let ay = axis.y / len;
                        let az = axis.z / len;
                        // If axis is aligned with X/Y/Z, simple
                        if ax.abs() > 0.99 {
                            Ok(Some(body.rotate(angle * ax.signum(), 0.0, 0.0)))
                        } else if ay.abs() > 0.99 {
                            Ok(Some(body.rotate(0.0, angle * ay.signum(), 0.0)))
                        } else if az.abs() > 0.99 {
                            Ok(Some(body.rotate(0.0, 0.0, angle * az.signum())))
                        } else {
                            // general axis: use Manifold rotate only supports euler; do ZYX approx
                            Ok(Some(body.rotate(angle * ax, angle * ay, angle * az)))
                        }
                    }
                } else {
                    let a = self.vec3_arg(&named, &positional, "a", 0)?;
                    Ok(Some(body.rotate(a.x, a.y, a.z)))
                }
            }
            "scale" => {
                let v = self.vec3_arg(&named, &positional, "v", 0)?;
                Ok(Some(union_all(self.eval_children(children)?).scale(v)))
            }
            "mirror" => {
                let v = self.vec3_arg(&named, &positional, "v", 0)?;
                Ok(Some(union_all(self.eval_children(children)?).mirror(v)))
            }
            "hull" => {
                let kids = self.eval_children(children)?;
                if kids.is_empty() {
                    return Ok(Some(Manifold::empty()));
                }
                Ok(Some({
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Manifold::hull_manifolds(&kids))) {
                        Ok(m) => m,
                        Err(_) => {
                            eprintln!("warning: hull failed (non-manifold); using union");
                            union_all(kids)
                        }
                    }
                }))
            }
            "minkowski" => {
                let kids = self.eval_children(children)?;
                if kids.is_empty() {
                    return Ok(Some(Manifold::empty()));
                }
                let mut it = kids.into_iter();
                let mut acc = it.next().unwrap();
                for p in it {
                    acc = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| acc.minkowski_sum(&p))) {
                        Ok(m) => m,
                        Err(_) => {
                            eprintln!("warning: minkowski failed; keeping left operand");
                            acc
                        }
                    };
                }
                Ok(Some(acc))
            }
            "linear_extrude" => Ok(Some(self.builtin_linear_extrude(&named, &positional, children)?)),
            "rotate_extrude" => Ok(Some(self.builtin_rotate_extrude(&named, &positional, children)?)),
            "group" | "color" | "render" | "debug" | "background" => {
                Ok(Some(union_all(self.eval_children(children)?)))
            }
            "echo" => {
                // no-op geometry
                Ok(None)
            }
            "children" => {
                // children() — not fully supported without parent stack
                Ok(None)
            }
            "text" => {
                bail!("text() is not implemented yet (no font engine); use linear_extrude+shapes instead")
            }
            other => bail!("unsupported module '{}'", other),
        }
    }

    fn call_user_module(
        &mut self,
        def: &ModuleDef,
        args: &[Arg],
        children: &[Statement],
    ) -> Result<Option<Manifold>> {
        let mut child = self.child_scope();
        // bind parameters
        let named = collect_named(args);
        let positional: Vec<&Expr> = args
            .iter()
            .filter_map(|a| match a {
                Arg::Positional(e) => Some(e),
                _ => None,
            })
            .collect();

        let mut pos_i = 0;
        for p in &def.params {
            if let Some(e) = named.get(&p.name) {
                child.vars.insert(p.name.clone(), self.eval_expr(e)?);
            } else if pos_i < positional.len() {
                child
                    .vars
                    .insert(p.name.clone(), self.eval_expr(positional[pos_i])?);
                pos_i += 1;
            } else if let Some(d) = &p.default {
                child.vars.insert(p.name.clone(), self.eval_expr(d)?);
            }
        }

        // children available as special? OpenSCAD uses children() — skip for now
        let _ = children;

        let mut parts = Vec::new();
        for s in &def.body {
            if let Some(m) = child.eval_statement(s)? {
                parts.push(m);
            }
        }
        Ok(Some(union_all(parts)))
    }

    fn eval_children(&mut self, children: &[Statement]) -> Result<Vec<Manifold>> {
        let mut out = Vec::new();
        for c in children {
            if let Some(m) = self.eval_statement(c)? {
                out.push(m);
            }
        }
        Ok(out)
    }

    // ---- builtins ----

    fn builtin_cube(
        &mut self,
        named: &HashMap<String, &Expr>,
        positional: &[&Expr],
    ) -> Result<Manifold> {
        let size = if let Some(e) = named.get("size") {
            self.eval_vec3(e)?
        } else if let Some(e) = positional.get(0) {
            match self.eval_expr(e)? {
                Value::Number(n) => Vec3::new(n, n, n),
                Value::Vec3(v) => v,
                Value::Vec2(x, y) => Vec3::new(x, y, 1.0),
                other => bail!("cube size: {:?}", other),
            }
        } else {
            Vec3::new(1.0, 1.0, 1.0)
        };
        let center = self.bool_arg(named, positional, "center", 1, false)?;
        Ok(Manifold::cube(size, center))
    }

    fn builtin_sphere(
        &mut self,
        named: &HashMap<String, &Expr>,
        positional: &[&Expr],
    ) -> Result<Manifold> {
        let r = if let Some(e) = named.get("r") {
            self.eval_number(e)?
        } else if let Some(e) = named.get("d") {
            self.eval_number(e)? / 2.0
        } else if let Some(e) = positional.get(0) {
            self.eval_number(e)?
        } else {
            1.0
        };
        let segments = if let Some(e) = named.get("$fn") {
            self.eval_number(e)?.round().max(3.0) as i32
        } else {
            self.segments_for_radius(r)
        };
        Ok(Manifold::sphere(r, segments))
    }

    fn builtin_cylinder(
        &mut self,
        named: &HashMap<String, &Expr>,
        positional: &[&Expr],
    ) -> Result<Manifold> {
        let h = if let Some(e) = named.get("h").or(named.get("height")) {
            self.eval_number(e)?
        } else if let Some(e) = positional.get(0) {
            self.eval_number(e)?
        } else {
            1.0
        };

        let (r_low, r_high) = if named.contains_key("r1") || named.contains_key("r2") {
            let r1 = named
                .get("r1")
                .map(|e| self.eval_number(e))
                .transpose()?
                .unwrap_or(1.0);
            let r2 = named
                .get("r2")
                .map(|e| self.eval_number(e))
                .transpose()?
                .unwrap_or(r1);
            (r1, r2)
        } else if let Some(e) = named.get("r") {
            let r = self.eval_number(e)?;
            (r, r)
        } else if let Some(e) = named.get("d") {
            let r = self.eval_number(e)? / 2.0;
            (r, r)
        } else if positional.len() >= 3 {
            (
                self.eval_number(positional[1])?,
                self.eval_number(positional[2])?,
            )
        } else if positional.len() >= 2 {
            let r = self.eval_number(positional[1])?;
            (r, r)
        } else {
            (1.0, 1.0)
        };

        let center = self.bool_arg(named, positional, "center", 3, false)?;
        let r_max = r_low.max(r_high);
        let segments = if let Some(e) = named.get("$fn") {
            self.eval_number(e)?.round().max(3.0) as i32
        } else {
            self.segments_for_radius(r_max)
        };

        if center {
            Ok(Manifold::cylinder_centered(
                h, r_low, r_high, segments, true,
            ))
        } else {
            Ok(Manifold::cylinder(h, r_low, r_high, segments))
        }
    }


    fn builtin_polygon(
        &mut self,
        named: &HashMap<String, &Expr>,
        positional: &[&Expr],
    ) -> Result<Manifold> {
        // polygon(points) or polygon(points, paths) — extrude thin
        let pe: &Expr = if let Some(e) = named.get("points") {
            e
        } else if let Some(e) = positional.first() {
            e
        } else {
            bail!("polygon() requires points");
        };
        let pts = match self.eval_expr(pe)? {
            Value::List(list) => {
                let mut out = Vec::new();
                for item in list {
                    match item {
                        Value::Vec2(x, y) => out.push(Vec2::new(x, y)),
                        Value::Vec3(v) => out.push(Vec2::new(v.x, v.y)),
                        Value::List(c) if c.len() >= 2 => {
                            out.push(Vec2::new(c[0].as_number()?, c[1].as_number()?));
                        }
                        _ => {}
                    }
                }
                out
            }
            _ => bail!("polygon points must be a list"),
        };
        if pts.len() < 3 {
            bail!("polygon needs >= 3 points");
        }
        {
            let polys = vec![pts];
            Ok(Manifold::extrude(&polys, 0.01, 1, 0.0, Vec2::new(1.0, 1.0)))
        }
    }

    fn builtin_polyhedron(
        &mut self,
        named: &HashMap<String, &Expr>,
        positional: &[&Expr],
    ) -> Result<Manifold> {
        // polyhedron(points, faces) — build MeshGL
        let pe: &Expr = if let Some(e) = named.get("points") {
            e
        } else if let Some(e) = positional.first() {
            e
        } else {
            bail!("polyhedron() requires points and faces");
        };
        let fe: &Expr = if let Some(e) = named.get("faces").or(named.get("triangles")) {
            e
        } else if positional.len() >= 2 {
            positional[1]
        } else {
            bail!("polyhedron() requires points and faces");
        };
        let mut verts: Vec<f32> = Vec::new();
        match self.eval_expr(pe)? {
            Value::List(list) => {
                for item in list {
                    let (x, y, z) = match item {
                        Value::Vec3(v) => (v.x, v.y, v.z),
                        Value::List(c) if c.len() >= 3 => {
                            (c[0].as_number()?, c[1].as_number()?, c[2].as_number()?)
                        }
                        Value::Vec2(x, y) => (x, y, 0.0),
                        _ => bail!("bad polyhedron point"),
                    };
                    verts.push(x as f32);
                    verts.push(y as f32);
                    verts.push(z as f32);
                }
            }
            _ => bail!("polyhedron points must be a list"),
        }
        let mut indices: Vec<u32> = Vec::new();
        match self.eval_expr(fe)? {
            Value::List(faces) => {
                for face in faces {
                    let idxs: Vec<u32> = match face {
                        Value::List(c) => c
                            .iter()
                            .map(|v| Ok(v.as_number()?.round() as u32))
                            .collect::<Result<_>>()?,
                        _ => bail!("bad face"),
                    };
                    // triangulate fan
                    if idxs.len() >= 3 {
                        for i in 1..idxs.len() - 1 {
                            indices.push(idxs[0]);
                            indices.push(idxs[i]);
                            indices.push(idxs[i + 1]);
                        }
                    }
                }
            }
            _ => bail!("polyhedron faces must be a list"),
        }
        // Convex hull of points (exact face mesh would need MeshGL construction)
        let mut pts3 = Vec::new();
        for i in 0..verts.len() / 3 {
            pts3.push(Vec3::new(
                verts[i * 3] as f64,
                verts[i * 3 + 1] as f64,
                verts[i * 3 + 2] as f64,
            ));
        }
        if pts3.is_empty() {
            return Ok(Manifold::empty());
        }
        // Convex hull of points as approximation when full mesh import is hard
        Ok(Manifold::hull(&pts3))
    }


    /// 2D circle as a very thin cylinder (OpenSCAD 2D-in-3D fallback)
    fn builtin_circle_2d(
        &mut self,
        named: &HashMap<String, &Expr>,
        positional: &[&Expr],
    ) -> Result<Manifold> {
        let r = if let Some(e) = named.get("r") {
            self.eval_number(e)?
        } else if let Some(e) = named.get("d") {
            self.eval_number(e)? / 2.0
        } else if let Some(e) = positional.get(0) {
            self.eval_number(e)?
        } else {
            1.0
        };
        let segments = if let Some(e) = named.get("$fn") {
            self.eval_number(e)?.round().max(3.0) as i32
        } else {
            self.segments_for_radius(r)
        };
        // thin disk in XY plane
        Ok(Manifold::cylinder(0.01, r, r, segments))
    }

    fn builtin_square_2d(
        &mut self,
        named: &HashMap<String, &Expr>,
        positional: &[&Expr],
    ) -> Result<Manifold> {
        let (x, y) = if let Some(e) = named.get("size") {
            match self.eval_expr(e)? {
                Value::Number(n) => (n, n),
                Value::Vec2(a, b) => (a, b),
                Value::Vec3(v) => (v.x, v.y),
                _ => (1.0, 1.0),
            }
        } else if let Some(e) = positional.get(0) {
            match self.eval_expr(e)? {
                Value::Number(n) => (n, n),
                Value::Vec2(a, b) => (a, b),
                Value::Vec3(v) => (v.x, v.y),
                _ => (1.0, 1.0),
            }
        } else {
            (1.0, 1.0)
        };
        let center = self.bool_arg(named, positional, "center", 1, false)?;
        Ok(Manifold::cube(Vec3::new(x, y, 0.01), center))
    }

    fn builtin_linear_extrude(

        &mut self,
        named: &HashMap<String, &Expr>,
        positional: &[&Expr],
        children: &[Statement],
    ) -> Result<Manifold> {
        // linear_extrude(height) children
        // For now: if children are 3D we just scale in Z as approximation is wrong.
        // Better path: treat 2D primitives specially. We support a simple path:
        // linear_extrude(h) { square / circle } via CrossSection.
        let height = if let Some(e) = named.get("height").or(named.get("h")) {
            self.eval_number(e)?
        } else if let Some(e) = positional.get(0) {
            self.eval_number(e)?
        } else {
            1.0
        };
        let twist = named
            .get("twist")
            .map(|e| self.eval_number(e))
            .transpose()?
            .unwrap_or(0.0);
        let slices = named
            .get("slices")
            .map(|e| self.eval_number(e))
            .transpose()?
            .unwrap_or(1.0)
            .round()
            .max(1.0) as i32;
        let scale = named
            .get("scale")
            .map(|e| self.eval_vec2(e))
            .transpose()?
            .unwrap_or(Vec2::new(1.0, 1.0));

        // Try to interpret children as 2D cross-sections via special names
        // square / circle at top level of children
        if let Some(cs) = self.try_children_as_cross_section(children)? {
            return Ok(Manifold::extrude(
                &cs,
                height,
                slices,
                twist,
                scale,
            ));
        }

        // Fallback: take 3D children and non-uniform scale Z (not true extrude)
        // Better than nothing for demos that nest 3D by mistake.
        let body = union_all(self.eval_children(children)?);
        Ok(body)
    }

    fn builtin_rotate_extrude(
        &mut self,
        named: &HashMap<String, &Expr>,
        positional: &[&Expr],
        children: &[Statement],
    ) -> Result<Manifold> {
        // rotate_extrude(angle=360) { 2D children }
        let angle = if let Some(e) = named.get("angle") {
            self.eval_number(e)?
        } else if let Some(e) = positional.get(0) {
            self.eval_number(e)?
        } else {
            360.0
        };
        let segments = if let Some(e) = named.get("$fn") {
            self.eval_number(e)?.round().max(3.0) as i32
        } else {
            // approximate from angle and $fa/$fs
            let fa = self.var_number("$fa").unwrap_or(12.0).max(0.01);
            ((angle.abs() / fa).ceil() as i32).max(3)
        };

        if let Some(cs) = self.try_children_as_cross_section(children)? {
            // Manifold::revolve(cross_section, circular_segments, revolve_degrees)
            return Ok(Manifold::revolve(&cs, segments, angle));
        }

        // Fallback: if children are already 3D, just return them (not ideal)
        Ok(union_all(self.eval_children(children)?))
    }

    fn try_children_as_cross_section(
        &mut self,
        children: &[Statement],
    ) -> Result<Option<Vec<Vec<manifold_rust::linalg::Vec2>>>> {
        // Only handle single square/circle module call for now
        if children.len() != 1 {
            return Ok(None);
        }
        if let Statement::ModuleCall { name, args, .. } = &children[0] {
            let named = collect_named(args);
            let positional: Vec<&Expr> = args
                .iter()
                .filter_map(|a| match a {
                    Arg::Positional(e) => Some(e),
                    _ => None,
                })
                .collect();
            match name.as_str() {
                "square" => {
                    let size = if let Some(e) = named.get("size") {
                        match self.eval_expr(e)? {
                            Value::Number(n) => (n, n),
                            Value::Vec2(x, y) => (x, y),
                            Value::Vec3(v) => (v.x, v.y),
                            _ => (1.0, 1.0),
                        }
                    } else if let Some(e) = positional.get(0) {
                        match self.eval_expr(e)? {
                            Value::Number(n) => (n, n),
                            Value::Vec2(x, y) => (x, y),
                            Value::Vec3(v) => (v.x, v.y),
                            _ => (1.0, 1.0),
                        }
                    } else {
                        (1.0, 1.0)
                    };
                    let center = self.bool_arg(&named, &positional, "center", 1, false)?;
                    let (x, y) = size;
                    let (ox, oy) = if center { (-x / 2.0, -y / 2.0) } else { (0.0, 0.0) };
                    let poly = vec![
                        Vec2::new(ox, oy),
                        Vec2::new(ox + x, oy),
                        Vec2::new(ox + x, oy + y),
                        Vec2::new(ox, oy + y),
                    ];
                    // Polygons type is Vec of contours
                    return Ok(Some(vec![poly]));
                }
                "circle" => {
                    let r = if let Some(e) = named.get("r") {
                        self.eval_number(e)?
                    } else if let Some(e) = named.get("d") {
                        self.eval_number(e)? / 2.0
                    } else if let Some(e) = positional.get(0) {
                        self.eval_number(e)?
                    } else {
                        1.0
                    };
                    let segs = self.segments_for_radius(r);
                    let mut poly = Vec::new();
                    for i in 0..segs {
                        let a = 2.0 * PI * (i as f64) / (segs as f64);
                        poly.push(Vec2::new(r * a.cos(), r * a.sin()));
                    }
                    return Ok(Some(vec![poly]));
                }
                _ => {}
            }
        }
        Ok(None)
    }

    // ---- expressions ----

    fn eval_expr(&mut self, expr: &Expr) -> Result<Value> {
        match expr {
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::String(s) => Ok(Value::String(s.clone())),
            Expr::Ident(name) => {
                if name == "undef" {
                    Ok(Value::Undef)
                } else if name == "PI" {
                    Ok(Value::Number(std::f64::consts::PI))
                } else if let Some(v) = self.vars.get(name) {
                    Ok(v.clone())
                } else {
                    // OpenSCAD treats unknown as undef in some contexts; we error
                    bail!("undefined identifier '{}'", name)
                }
            }
            Expr::Vector(elems) => {
                let mut nums = Vec::new();
                for e in elems {
                    match self.eval_expr(e)? {
                        Value::Number(n) => nums.push(n),
                        Value::List(list) => {
                            // flatten one level of numbers
                            for v in list {
                                if let Value::Number(n) = v {
                                    nums.push(n);
                                }
                            }
                        }
                        other => bail!("vector element must be number, got {:?}", other),
                    }
                }
                match nums.len() {
                    2 => Ok(Value::Vec2(nums[0], nums[1])),
                    3 => Ok(Value::Vec3(Vec3::new(nums[0], nums[1], nums[2]))),
                    _ => Ok(Value::List(
                        nums.into_iter().map(Value::Number).collect(),
                    )),
                }
            }
            Expr::Range { start, step, end } => {
                let s = self.eval_number(start)?;
                let e = self.eval_number(end)?;
                let st = if let Some(st) = step {
                    self.eval_number(st)?
                } else {
                    if e >= s {
                        1.0
                    } else {
                        -1.0
                    }
                };
                if st == 0.0 {
                    bail!("range step cannot be 0");
                }
                let mut list = Vec::new();
                let mut x = s;
                if st > 0.0 {
                    while x <= e + 1e-12 {
                        list.push(Value::Number(x));
                        x += st;
                    }
                } else {
                    while x >= e - 1e-12 {
                        list.push(Value::Number(x));
                        x += st;
                    }
                }
                Ok(Value::List(list))
            }
            Expr::ListComp { name, range, body } => {
                let values = self.eval_range_or_list(range)?;
                let mut out = Vec::new();
                for v in values {
                    let mut child = self.child_scope();
                    child.vars.insert(name.clone(), v);
                    out.push(child.eval_expr(body)?);
                }
                Ok(Value::List(out))
            }
            Expr::Call { name, args } => self.eval_call(name, args),
            Expr::Index { base, index } => {
                let b = self.eval_expr(base)?;
                let i = self.eval_number(index)?.round() as i64;
                match b {
                    Value::List(list) => {
                        let idx = if i < 0 {
                            list.len() as i64 + i
                        } else {
                            i
                        };
                        if idx < 0 || idx as usize >= list.len() {
                            bail!("index {} out of bounds (len {})", i, list.len());
                        }
                        Ok(list[idx as usize].clone())
                    }
                    Value::Vec2(x, y) => match i {
                        0 => Ok(Value::Number(x)),
                        1 => Ok(Value::Number(y)),
                        _ => bail!("vec2 index out of bounds"),
                    },
                    Value::Vec3(v) => match i {
                        0 => Ok(Value::Number(v.x)),
                        1 => Ok(Value::Number(v.y)),
                        2 => Ok(Value::Number(v.z)),
                        _ => bail!("vec3 index out of bounds"),
                    },
                    other => bail!("cannot index {:?}", other),
                }
            }
            Expr::Unary { op, expr } => match op {
                UnaryOp::Neg => Ok(Value::Number(-self.eval_number(expr)?)),
                UnaryOp::Not => Ok(Value::Bool(!self.eval_bool(expr)?)),
            },
            Expr::Binary { op, left, right } => self.eval_binary(*op, left, right),
            Expr::Ternary {
                cond,
                then_expr,
                else_expr,
            } => {
                if self.eval_bool(cond)? {
                    self.eval_expr(then_expr)
                } else {
                    self.eval_expr(else_expr)
                }
            }
        }
    }

    fn eval_call(&mut self, name: &str, args: &[Arg]) -> Result<Value> {
        // user function
        if let Some(def) = self.functions.get(name).cloned() {
            let mut child = self.child_scope();
            let named = collect_named(args);
            let positional: Vec<&Expr> = args
                .iter()
                .filter_map(|a| match a {
                    Arg::Positional(e) => Some(e),
                    _ => None,
                })
                .collect();
            let mut pos_i = 0;
            for p in &def.params {
                if let Some(e) = named.get(&p.name) {
                    child.vars.insert(p.name.clone(), self.eval_expr(e)?);
                } else if pos_i < positional.len() {
                    child
                        .vars
                        .insert(p.name.clone(), self.eval_expr(positional[pos_i])?);
                    pos_i += 1;
                } else if let Some(d) = &p.default {
                    child.vars.insert(p.name.clone(), self.eval_expr(d)?);
                }
            }
            return child.eval_expr(&def.body);
        }

        // builtin functions
        let positional: Vec<Value> = args
            .iter()
            .filter_map(|a| match a {
                Arg::Positional(e) => Some(e),
                _ => None,
            })
            .map(|e| self.eval_expr(e))
            .collect::<Result<_>>()?;

        match name {
            "abs" => Ok(Value::Number(positional[0].as_number()?.abs())),
            "sign" => {
                let n = positional[0].as_number()?;
                Ok(Value::Number(if n > 0.0 { 1.0 } else if n < 0.0 { -1.0 } else { 0.0 }))
            }
            "ceil" => Ok(Value::Number(positional[0].as_number()?.ceil())),
            "floor" => Ok(Value::Number(positional[0].as_number()?.floor())),
            "round" => Ok(Value::Number(positional[0].as_number()?.round())),
            "sqrt" => Ok(Value::Number(positional[0].as_number()?.sqrt())),
            "pow" => Ok(Value::Number(positional[0].as_number()?.powf(positional[1].as_number()?))),
            "exp" => Ok(Value::Number(positional[0].as_number()?.exp())),
            "ln" | "log" => {
                // OpenSCAD log() is natural log; log(x,b) is log base b
                if positional.len() >= 2 {
                    let x = positional[0].as_number()?;
                    let b = positional[1].as_number()?;
                    Ok(Value::Number(x.ln() / b.ln()))
                } else {
                    Ok(Value::Number(positional[0].as_number()?.ln()))
                }
            }
            "sin" => Ok(Value::Number(positional[0].as_number()?.to_radians().sin())),
            "cos" => Ok(Value::Number(positional[0].as_number()?.to_radians().cos())),
            "tan" => Ok(Value::Number(positional[0].as_number()?.to_radians().tan())),
            "asin" => Ok(Value::Number(positional[0].as_number()?.asin().to_degrees())),
            "acos" => Ok(Value::Number(positional[0].as_number()?.acos().to_degrees())),
            "atan" => Ok(Value::Number(positional[0].as_number()?.atan().to_degrees())),
            "atan2" => Ok(Value::Number(
                positional[0].as_number()?.atan2(positional[1].as_number()?).to_degrees(),
            )),
            "min" => {
                let mut m = f64::INFINITY;
                for v in &positional {
                    m = m.min(v.as_number()?);
                }
                Ok(Value::Number(m))
            }
            "max" => {
                let mut m = f64::NEG_INFINITY;
                for v in &positional {
                    m = m.max(v.as_number()?);
                }
                Ok(Value::Number(m))
            }
            "norm" => {
                // norm(v) = length of vector
                match &positional[0] {
                    Value::Number(n) => Ok(Value::Number(n.abs())),
                    Value::Vec2(x, y) => Ok(Value::Number((x * x + y * y).sqrt())),
                    Value::Vec3(v) => Ok(Value::Number((v.x * v.x + v.y * v.y + v.z * v.z).sqrt())),
                    Value::List(list) => {
                        let mut s = 0.0;
                        for item in list {
                            let n = item.as_number()?;
                            s += n * n;
                        }
                        Ok(Value::Number(s.sqrt()))
                    }
                    other => bail!("norm() expected vector, got {:?}", other),
                }
            }
            "cross" => {
                let a = match &positional[0] {
                    Value::Vec3(v) => *v,
                    Value::List(l) if l.len() >= 3 => Vec3::new(
                        l[0].as_number()?,
                        l[1].as_number()?,
                        l[2].as_number()?,
                    ),
                    _ => bail!("cross() expected 3-vectors"),
                };
                let b = match &positional[1] {
                    Value::Vec3(v) => *v,
                    Value::List(l) if l.len() >= 3 => Vec3::new(
                        l[0].as_number()?,
                        l[1].as_number()?,
                        l[2].as_number()?,
                    ),
                    _ => bail!("cross() expected 3-vectors"),
                };
                Ok(Value::Vec3(Vec3::new(
                    a.y * b.z - a.z * b.y,
                    a.z * b.x - a.x * b.z,
                    a.x * b.y - a.y * b.x,
                )))
            }
            "dot" | "scalar_product" => {
                fn as_comps(v: &Value) -> Result<Vec<f64>> {
                    match v {
                        Value::Number(n) => Ok(vec![*n]),
                        Value::Vec2(x, y) => Ok(vec![*x, *y]),
                        Value::Vec3(v) => Ok(vec![v.x, v.y, v.z]),
                        Value::List(l) => l.iter().map(|x| x.as_number()).collect(),
                        other => bail!("dot() bad arg {:?}", other),
                    }
                }
                let a = as_comps(&positional[0])?;
                let b = as_comps(&positional[1])?;
                if a.len() != b.len() {
                    bail!("dot() length mismatch");
                }
                Ok(Value::Number(a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()))
            }
            "concat" => {
                let mut out = Vec::new();
                for v in &positional {
                    match v {
                        Value::List(l) => out.extend(l.iter().cloned()),
                        other => out.push(other.clone()),
                    }
                }
                Ok(Value::List(out))
            }
            "lookup" => {
                // lookup(key, [[k,v],...]) — linear interpolation
                let key = positional[0].as_number()?;
                let table = match &positional[1] {
                    Value::List(l) => l,
                    _ => bail!("lookup() expected table list"),
                };
                let mut pairs: Vec<(f64, f64)> = Vec::new();
                for row in table {
                    match row {
                        Value::List(r) if r.len() >= 2 => {
                            pairs.push((r[0].as_number()?, r[1].as_number()?));
                        }
                        Value::Vec2(k, v) => pairs.push((*k, *v)),
                        _ => {}
                    }
                }
                pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                if pairs.is_empty() {
                    return Ok(Value::Number(0.0));
                }
                if key <= pairs[0].0 {
                    return Ok(Value::Number(pairs[0].1));
                }
                if key >= pairs.last().unwrap().0 {
                    return Ok(Value::Number(pairs.last().unwrap().1));
                }
                for w in pairs.windows(2) {
                    if key >= w[0].0 && key <= w[1].0 {
                        let t = (key - w[0].0) / (w[1].0 - w[0].0);
                        return Ok(Value::Number(w[0].1 + t * (w[1].1 - w[0].1)));
                    }
                }
                Ok(Value::Number(0.0))
            }
            "rands" => {
                // rands(min, max, n [, seed]) — deterministic pseudo if seed given
                let lo = positional[0].as_number()?;
                let hi = positional[1].as_number()?;
                let n = positional[2].as_number()?.round().max(0.0) as usize;
                let mut seed = if positional.len() >= 4 {
                    positional[3].as_number()? as u64
                } else {
                    1u64
                };
                let mut out = Vec::with_capacity(n);
                for _ in 0..n {
                    // simple LCG
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    let u = (seed >> 33) as f64 / (1u64 << 31) as f64;
                    out.push(Value::Number(lo + (hi - lo) * u));
                }
                Ok(Value::List(out))
            }
            "len" => match &positional[0] {
                Value::List(l) => Ok(Value::Number(l.len() as f64)),
                Value::String(s) => Ok(Value::Number(s.len() as f64)),
                Value::Vec2(_, _) => Ok(Value::Number(2.0)),
                Value::Vec3(_) => Ok(Value::Number(3.0)),
                _ => bail!("len() on non-list"),
            },
            "str" => {
                let s = positional
                    .iter()
                    .map(|v| match v {
                        Value::Number(n) => format!("{}", n),
                        Value::Bool(b) => format!("{}", b),
                        Value::String(s) => s.clone(),
                        other => format!("{:?}", other),
                    })
                    .collect::<Vec<_>>()
                    .join("");
                Ok(Value::String(s))
            }
            "chr" => {
                let n = positional[0].as_number()?.round() as u32;
                Ok(Value::String(
                    std::char::from_u32(n).map(|c| c.to_string()).unwrap_or_default(),
                ))
            }
            "ord" => {
                let s = match &positional[0] {
                    Value::String(s) => s.clone(),
                    _ => bail!("ord() expected string"),
                };
                Ok(Value::Number(
                    s.chars().next().map(|c| c as u32 as f64).unwrap_or(0.0),
                ))
            }
            "is_undef" => Ok(Value::Bool(matches!(positional.get(0), Some(Value::Undef) | None))),
            "is_bool" => Ok(Value::Bool(matches!(positional.get(0), Some(Value::Bool(_))))),
            "is_num" => Ok(Value::Bool(matches!(positional.get(0), Some(Value::Number(_))))),
            "is_string" => Ok(Value::Bool(matches!(positional.get(0), Some(Value::String(_))))),
            "is_list" => Ok(Value::Bool(matches!(
                positional.get(0),
                Some(Value::List(_)) | Some(Value::Vec2(_, _)) | Some(Value::Vec3(_))
            ))),
            "is_function" => Ok(Value::Bool(false)), // anonymous functions not stored as values yet
            "version" => Ok(Value::List(vec![
                Value::Number(2021.0),
                Value::Number(1.0),
                Value::Number(0.0),
            ])),
            "version_num" => Ok(Value::Number(20210100.0)),
            "parent_module" => Ok(Value::String("openscad_rs".into())),
            "search" => {
                // search(match, string|list) — simplified
                if positional.len() < 2 {
                    bail!("search() needs 2 args");
                }
                Ok(Value::List(vec![])) // stub: empty matches
            }
            other => bail!("unknown function '{}'", other),

        }
    }

    fn eval_binary(&mut self, op: BinaryOp, left: &Expr, right: &Expr) -> Result<Value> {
        // short-circuit for and/or
        match op {
            BinaryOp::And => {
                return Ok(Value::Bool(
                    self.eval_bool(left)? && self.eval_bool(right)?,
                ))
            }
            BinaryOp::Or => {
                return Ok(Value::Bool(
                    self.eval_bool(left)? || self.eval_bool(right)?,
                ))
            }
            _ => {}
        }
        let a = self.eval_expr(left)?;
        let b = self.eval_expr(right)?;
        match op {
            BinaryOp::Add => match (&a, &b) {
                (Value::Number(x), Value::Number(y)) => Ok(Value::Number(x + y)),
                (Value::Vec3(u), Value::Vec3(v)) => {
                    Ok(Value::Vec3(Vec3::new(u.x + v.x, u.y + v.y, u.z + v.z)))
                }
                (Value::Vec2(x1, y1), Value::Vec2(x2, y2)) => {
                    Ok(Value::Vec2(x1 + x2, y1 + y2))
                }
                (Value::Number(s), Value::Vec3(v)) | (Value::Vec3(v), Value::Number(s)) => {
                    Ok(Value::Vec3(Vec3::new(v.x + s, v.y + s, v.z + s)))
                }
                (Value::List(l), Value::List(r)) if l.len() == r.len() => {
                    let mut out = Vec::new();
                    for (x, y) in l.iter().zip(r.iter()) {
                        out.push(Value::Number(x.as_number()? + y.as_number()?));
                    }
                    Ok(list_to_vec_value(out))
                }
                _ => bail!("cannot add {:?} and {:?}", a, b),
            },
            BinaryOp::Sub => match (&a, &b) {
                (Value::Number(x), Value::Number(y)) => Ok(Value::Number(x - y)),
                (Value::Vec3(u), Value::Vec3(v)) => {
                    Ok(Value::Vec3(Vec3::new(u.x - v.x, u.y - v.y, u.z - v.z)))
                }
                (Value::Vec2(x1, y1), Value::Vec2(x2, y2)) => {
                    Ok(Value::Vec2(x1 - x2, y1 - y2))
                }
                (Value::Vec3(v), Value::Number(s)) => {
                    Ok(Value::Vec3(Vec3::new(v.x - s, v.y - s, v.z - s)))
                }
                (Value::List(l), Value::List(r)) if l.len() == r.len() => {
                    let mut out = Vec::new();
                    for (x, y) in l.iter().zip(r.iter()) {
                        out.push(Value::Number(x.as_number()? - y.as_number()?));
                    }
                    Ok(list_to_vec_value(out))
                }
                _ => bail!("cannot sub {:?} and {:?}", a, b),
            },
            BinaryOp::Mul => match (&a, &b) {
                (Value::Number(x), Value::Number(y)) => Ok(Value::Number(x * y)),
                (Value::Number(s), Value::Vec3(v)) | (Value::Vec3(v), Value::Number(s)) => {
                    Ok(Value::Vec3(Vec3::new(v.x * s, v.y * s, v.z * s)))
                }
                (Value::Number(s), Value::Vec2(x, y)) | (Value::Vec2(x, y), Value::Number(s)) => {
                    Ok(Value::Vec2(x * s, y * s))
                }
                (Value::Vec3(u), Value::Vec3(v)) => {
                    // component-wise (OpenSCAD style for some ops)
                    Ok(Value::Vec3(Vec3::new(u.x * v.x, u.y * v.y, u.z * v.z)))
                }
                (Value::List(l), Value::Number(s)) | (Value::Number(s), Value::List(l)) => {
                    let out: Result<Vec<_>> = l.iter().map(|x| Ok(Value::Number(x.as_number()? * s))).collect();
                    Ok(list_to_vec_value(out?))
                }
                _ => bail!("cannot mul {:?} and {:?}", a, b),
            },
            BinaryOp::Div => match (&a, &b) {
                (Value::Number(x), Value::Number(y)) => Ok(Value::Number(x / y)),
                (Value::Vec3(v), Value::Number(s)) => {
                    Ok(Value::Vec3(Vec3::new(v.x / s, v.y / s, v.z / s)))
                }
                (Value::Vec2(x, y), Value::Number(s)) => Ok(Value::Vec2(x / s, y / s)),
                (Value::List(l), Value::Number(s)) => {
                    let out: Result<Vec<_>> = l.iter().map(|x| Ok(Value::Number(x.as_number()? / s))).collect();
                    Ok(list_to_vec_value(out?))
                }
                (Value::Vec3(u), Value::Vec3(v)) => {
                    Ok(Value::Vec3(Vec3::new(u.x / v.x, u.y / v.y, u.z / v.z)))
                }
                _ => bail!("cannot div {:?} and {:?}", a, b),
            },
            BinaryOp::Mod => Ok(Value::Number(a.as_number()? % b.as_number()?)),
            BinaryOp::Pow => Ok(Value::Number(a.as_number()?.powf(b.as_number()?))),
            BinaryOp::Eq => Ok(Value::Bool(values_eq(&a, &b))),
            BinaryOp::Ne => Ok(Value::Bool(!values_eq(&a, &b))),
            BinaryOp::Lt => Ok(Value::Bool(a.as_number()? < b.as_number()?)),
            BinaryOp::Le => Ok(Value::Bool(a.as_number()? <= b.as_number()?)),
            BinaryOp::Gt => Ok(Value::Bool(a.as_number()? > b.as_number()?)),
            BinaryOp::Ge => Ok(Value::Bool(a.as_number()? >= b.as_number()?)),
            BinaryOp::And | BinaryOp::Or => unreachable!(),
        }
    }

    fn eval_range_or_list(&mut self, expr: &Expr) -> Result<Vec<Value>> {
        match self.eval_expr(expr)? {
            Value::List(l) => Ok(l),
            Value::Number(n) => Ok(vec![Value::Number(n)]),
            other => bail!("for-range expected list/range, got {:?}", other),
        }
    }

    fn eval_number(&mut self, expr: &Expr) -> Result<f64> {
        self.eval_expr(expr)?.as_number()
    }

    fn eval_bool(&mut self, expr: &Expr) -> Result<bool> {
        match self.eval_expr(expr)? {
            Value::Bool(b) => Ok(b),
            Value::Number(n) => Ok(n != 0.0),
            Value::List(l) => Ok(!l.is_empty()),
            _ => Ok(false),
        }
    }

    fn eval_vec3(&mut self, expr: &Expr) -> Result<Vec3> {
        match self.eval_expr(expr)? {
            Value::Number(n) => Ok(Vec3::new(n, n, n)),
            Value::Vec2(x, y) => Ok(Vec3::new(x, y, 0.0)),
            Value::Vec3(v) => Ok(v),
            other => bail!("expected vec3, got {:?}", other),
        }
    }

    fn eval_vec2(&mut self, expr: &Expr) -> Result<Vec2> {
        match self.eval_expr(expr)? {
            Value::Number(n) => Ok(Vec2::new(n, n)),
            Value::Vec2(x, y) => Ok(Vec2::new(x, y)),
            Value::Vec3(v) => Ok(Vec2::new(v.x, v.y)),
            other => bail!("expected vec2, got {:?}", other),
        }
    }

    fn vec3_arg(
        &mut self,
        named: &HashMap<String, &Expr>,
        positional: &[&Expr],
        name: &str,
        pos: usize,
    ) -> Result<Vec3> {
        if let Some(e) = named.get(name) {
            return self.eval_vec3(e);
        }
        if let Some(e) = positional.get(pos) {
            return self.eval_vec3(e);
        }
        bail!("missing argument '{}'", name)
    }

    fn bool_arg(
        &mut self,
        named: &HashMap<String, &Expr>,
        positional: &[&Expr],
        name: &str,
        pos: usize,
        default: bool,
    ) -> Result<bool> {
        if let Some(e) = named.get(name) {
            return self.eval_bool(e);
        }
        if let Some(e) = positional.get(pos) {
            return self.eval_bool(e);
        }
        Ok(default)
    }
}

#[derive(Debug, Clone)]
enum Value {
    Number(f64),
    Bool(bool),
    String(String),
    Vec2(f64, f64),
    Vec3(Vec3),
    List(Vec<Value>),
    Undef,
}

impl Value {
    fn as_number(&self) -> Result<f64> {
        match self {
            Value::Number(n) => Ok(*n),
            Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Value::Undef => bail!("expected number, got undef"),
            other => bail!("expected number, got {:?}", other),
        }
    }
}

fn values_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => (x - y).abs() < 1e-12,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Undef, Value::Undef) => true,
        _ => false,
    }
}

fn list_to_vec_value(nums: Vec<Value>) -> Value {
    let as_nums: Option<Vec<f64>> = nums.iter().map(|v| match v {
        Value::Number(n) => Some(*n),
        _ => None,
    }).collect();
    match as_nums {
        Some(n) if n.len() == 2 => Value::Vec2(n[0], n[1]),
        Some(n) if n.len() == 3 => Value::Vec3(Vec3::new(n[0], n[1], n[2])),
        _ => Value::List(nums),
    }
}

fn collect_named<'a>(args: &'a [Arg]) -> HashMap<String, &'a Expr> {
    let mut m = HashMap::new();
    for a in args {
        if let Arg::Named(k, v) = a {
            m.insert(k.clone(), v);
        }
    }
    m
}

fn is_empty_manifold(m: &Manifold) -> bool {
    m.num_tri() == 0
}

fn safe_union(a: Manifold, b: &Manifold) -> Manifold {
    if is_empty_manifold(b) {
        return a;
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| a.union(b))) {
        Ok(m) => m,
        Err(_) => {
            eprintln!("warning: union produced non-manifold geometry; skipping operand");
            a
        }
    }
}

fn safe_difference(a: Manifold, b: &Manifold) -> Manifold {
    if is_empty_manifold(&a) {
        return Manifold::empty();
    }
    if is_empty_manifold(b) {
        return a;
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| a.difference(b))) {
        Ok(m) => m,
        Err(_) => {
            eprintln!("warning: difference produced non-manifold geometry; keeping original");
            a
        }
    }
}

fn safe_intersection(a: Manifold, b: &Manifold) -> Manifold {
    if is_empty_manifold(&a) || is_empty_manifold(b) {
        return Manifold::empty();
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| a.intersection(b))) {
        Ok(m) => m,
        Err(_) => {
            eprintln!("warning: intersection produced non-manifold geometry; returning empty");
            Manifold::empty()
        }
    }
}

fn union_all(parts: Vec<Manifold>) -> Manifold {
    let parts: Vec<_> = parts.into_iter().filter(|m| !is_empty_manifold(m)).collect();
    if parts.is_empty() {
        return Manifold::empty();
    }
    let mut it = parts.into_iter();
    let mut acc = it.next().unwrap();
    for p in it {
        acc = safe_union(acc, &p);
    }
    acc
}

fn difference_all(parts: Vec<Manifold>) -> Manifold {
    if parts.is_empty() {
        return Manifold::empty();
    }
    let mut it = parts.into_iter();
    let mut acc = it.next().unwrap();
    if is_empty_manifold(&acc) {
        return Manifold::empty();
    }
    for p in it {
        if !is_empty_manifold(&p) {
            acc = safe_difference(acc, &p);
        }
    }
    acc
}

fn intersection_all(parts: Vec<Manifold>) -> Manifold {
    let parts: Vec<_> = parts.into_iter().filter(|m| !is_empty_manifold(m)).collect();
    if parts.is_empty() {
        return Manifold::empty();
    }
    let mut it = parts.into_iter();
    let mut acc = it.next().unwrap();
    for p in it {
        acc = safe_intersection(acc, &p);
    }
    acc
}
