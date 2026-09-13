//! OpenSCAD-like abstract syntax tree.

use std::fmt;

#[derive(Debug, Clone)]
pub struct Program {
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone)]
pub enum Statement {
    /// name = expr;
    Assign {
        name: String,
        value: Expr,
    },
    /// module name(params) body
    ModuleDef {
        name: String,
        params: Vec<Param>,
        body: Vec<Statement>,
    },
    /// function name(params) = expr;
    FunctionDef {
        name: String,
        params: Vec<Param>,
        body: Expr,
    },
    /// module_name(args) children
    ModuleCall {
        name: String,
        args: Vec<Arg>,
        children: Vec<Statement>,
    },
    /// if (cond) stmt [else stmt]
    If {
        cond: Expr,
        then_branch: Box<Statement>,
        else_branch: Option<Box<Statement>>,
    },
    /// for (name = range) stmt
    For {
        name: String,
        range: Expr,
        body: Box<Statement>,
    },
    /// Bare block { ... }
    Block(Vec<Statement>),
    /// expr;  (echo / ignore non-geometry)
    ExprStmt(Expr),
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone)]
pub enum Arg {
    Named(String, Expr),
    Positional(Expr),
}

#[derive(Debug, Clone)]
pub enum Expr {
    Number(f64),
    Bool(bool),
    String(String),
    /// [a, b, c] or list comprehension later simplified as vector
    Vector(Vec<Expr>),
    /// [start : end] or [start : step : end]
    Range {
        start: Box<Expr>,
        step: Option<Box<Expr>>,
        end: Box<Expr>,
    },
    Ident(String),
    /// name(args)
    Call {
        name: String,
        args: Vec<Arg>,
    },
    /// list[index]
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// condition ? then : else
    Ternary {
        cond: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
    /// [ for (x = r) expr ]  simplified list comprehension
    ListComp {
        name: String,
        range: Box<Expr>,
        body: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Number(n) => write!(f, "{}", n),
            Expr::Bool(b) => write!(f, "{}", b),
            Expr::String(s) => write!(f, "\"{}\"", s),
            Expr::Vector(v) => {
                write!(f, "[")?;
                for (i, e) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", e)?;
                }
                write!(f, "]")
            }
            Expr::Range { start, step, end } => {
                if let Some(s) = step {
                    write!(f, "[{}:{}:{}]", start, s, end)
                } else {
                    write!(f, "[{}:{}]", start, end)
                }
            }
            Expr::Ident(s) => write!(f, "{}", s),
            Expr::Call { name, args } => {
                write!(f, "{}(...{} args)", name, args.len())
            }
            Expr::Index { base, index } => write!(f, "{}[{}]", base, index),
            Expr::Unary { op, expr } => match op {
                UnaryOp::Neg => write!(f, "-{}", expr),
                UnaryOp::Not => write!(f, "!{}", expr),
            },
            Expr::Binary { op, left, right } => {
                write!(f, "({} {:?} {})", left, op, right)
            }
            Expr::Ternary {
                cond,
                then_expr,
                else_expr,
            } => write!(f, "({} ? {} : {})", cond, then_expr, else_expr),
            Expr::ListComp { name, range, body } => {
                write!(f, "[for ({} = {}) {}]", name, range, body)
            }
        }
    }
}
