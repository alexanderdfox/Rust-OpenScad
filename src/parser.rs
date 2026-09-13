//! Recursive-descent parser for an expanded OpenSCAD subset.

use crate::ast::*;
use anyhow::{bail, Result};

pub fn parse(source: &str) -> Result<Program> {
    let mut p = Parser::new(source);
    p.parse_program()
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            src: s.as_bytes(),
            pos: 0,
        }
    }

    
    fn loc(&self) -> (usize, usize, String) {
        let mut line = 1usize;
        let mut col = 1usize;
        for (i, &b) in self.src.iter().enumerate() {
            if i >= self.pos {
                break;
            }
            if b == b'\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        // context line
        let start = self.src[..self.pos.min(self.src.len())]
            .iter()
            .rposition(|&b| b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let end = self.src[self.pos.min(self.src.len())..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|i| self.pos + i)
            .unwrap_or(self.src.len());
        let snippet = String::from_utf8_lossy(&self.src[start..end.min(self.src.len())]).to_string();
        (line, col, snippet)
    }

    fn err(&self, msg: &str) -> anyhow::Error {
        let (line, col, snippet) = self.loc();
        anyhow::anyhow!("{} at line {}, column {}\n  | {}\n  | {:>width$}^", msg, line, col, snippet.trim_end(), "", width = col.saturating_sub(1))
    }

    fn parse_program(&mut self) -> Result<Program> {
        self.skip();
        let mut statements = Vec::new();
        while !self.eof() {
            statements.push(self.parse_statement()?);
            self.skip();
        }
        Ok(Program { statements })
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        self.skip();
        if self.peek() == Some(b'{') {
            return self.parse_block();
        }

        // include <file> / use <file> — accepted and ignored for now
        if self.peek_keyword("include") || self.peek_keyword("use") {
            return self.parse_include_or_use();
        }

        // OpenSCAD modifiers: % * ! # before a statement
        if matches!(self.peek(), Some(b'%') | Some(b'*') | Some(b'!') | Some(b'#')) {
            self.bump();
            self.skip();
            // re-parse the underlying statement (modifier ignored for geometry)
            return self.parse_statement();
        }

        // module / function definitions
        if self.peek_keyword("module") {
            return self.parse_module_def();
        }
        if self.peek_keyword("function") {
            return self.parse_function_def();
        }
        if self.peek_keyword("if") {
            return self.parse_if();
        }
        if self.peek_keyword("for")
            || self.peek_keyword("intersection_for")
            || self.peek_keyword("union_for")
        {
            return self.parse_for_any();
        }
        if self.peek_keyword("let") || self.peek_keyword("assign") {
            return self.parse_let();
        }
        if self.peek_keyword("assert") {
            return self.parse_assert();
        }

        // assignment: ident = expr ;
        // or module call: ident(...)
        if self.peek_is_ident() {
            let save = self.pos;
            let name = self.parse_ident()?;
            self.skip();
            if self.peek() == Some(b'=') {
                self.bump();
                let value = self.parse_expr()?;
                self.skip();
                if self.peek() == Some(b';') {
                    self.bump();
                }
                return Ok(Statement::Assign { name, value });
            }
            // module call
            self.pos = save;
            return self.parse_module_call();
        }

        // empty statement
        if self.peek() == Some(b';') {
            self.bump();
            return Ok(Statement::Block(vec![]));
        }

        // expression statement
        let e = self.parse_expr()?;
        self.skip();
        if self.peek() == Some(b';') {
            self.bump();
        }
        Ok(Statement::ExprStmt(e))
    }


    fn parse_include_or_use(&mut self) -> Result<Statement> {
        // include <...> or use <...> or include "..."
        if self.peek_keyword("include") {
            self.expect_keyword("include")?;
        } else {
            self.expect_keyword("use")?;
        }
        self.skip();
        if self.peek() == Some(b'<') {
            self.bump();
            while !self.eof() && self.peek() != Some(b'>') {
                self.bump();
            }
            self.expect(b'>')?;
        } else if self.peek() == Some(b'"') {
            let _ = self.parse_string()?;
        } else {
            return Err(self.err("expected <path> or \"path\" after include/use"));
        }
        self.skip();
        if self.peek() == Some(b';') {
            self.bump();
        }
        // no-op statement
        Ok(Statement::Block(vec![]))
    }

    fn parse_let(&mut self) -> Result<Statement> {
        // let (a=1, b=2) statement
        if self.peek_keyword("assign") {
            self.expect_keyword("assign")?;
        } else {
            self.expect_keyword("let")?;
        }
        self.skip();
        self.expect(b'(')?;
        // parse assignments as a block of Assign then body
        let mut assigns = Vec::new();
        self.skip();
        while self.peek() != Some(b')') && !self.eof() {
            let name = self.parse_ident()?;
            self.skip();
            self.expect(b'=')?;
            let value = self.parse_expr()?;
            assigns.push(Statement::Assign { name, value });
            self.skip();
            if self.peek() == Some(b',') {
                self.bump();
                self.skip();
            } else {
                break;
            }
        }
        self.expect(b')')?;
        self.skip();
        let body = self.parse_statement()?;
        assigns.push(body);
        Ok(Statement::Block(assigns))
    }

    fn parse_assert(&mut self) -> Result<Statement> {
        // assert(cond); or assert(cond, "msg"); — ignored
        self.expect_keyword("assert")?;
        self.skip();
        self.expect(b'(')?;
        let _ = self.parse_expr()?;
        self.skip();
        if self.peek() == Some(b',') {
            self.bump();
            let _ = self.parse_expr()?;
        }
        self.expect(b')')?;
        self.skip();
        if self.peek() == Some(b';') {
            self.bump();
        }
        Ok(Statement::Block(vec![]))
    }

    fn parse_module_def(&mut self) -> Result<Statement> {
        self.expect_keyword("module")?;
        self.skip();
        let name = self.parse_ident()?;
        self.skip();
        self.expect(b'(')?;
        let params = self.parse_params()?;
        self.expect(b')')?;
        self.skip();
        let body = match self.parse_statement()? {
            Statement::Block(stmts) => stmts,
            other => vec![other],
        };
        Ok(Statement::ModuleDef { name, params, body })
    }

    fn parse_function_def(&mut self) -> Result<Statement> {
        self.expect_keyword("function")?;
        self.skip();
        let name = self.parse_ident()?;
        self.skip();
        self.expect(b'(')?;
        let params = self.parse_params()?;
        self.expect(b')')?;
        self.skip();
        self.expect(b'=')?;
        let body = self.parse_expr()?;
        self.skip();
        if self.peek() == Some(b';') {
            self.bump();
        }
        Ok(Statement::FunctionDef { name, params, body })
    }

    fn parse_params(&mut self) -> Result<Vec<Param>> {
        self.skip();
        let mut params = Vec::new();
        if self.peek() == Some(b')') {
            return Ok(params);
        }
        loop {
            let name = self.parse_ident()?;
            self.skip();
            let default = if self.peek() == Some(b'=') {
                self.bump();
                Some(self.parse_expr()?)
            } else {
                None
            };
            params.push(Param { name, default });
            self.skip();
            if self.peek() == Some(b',') {
                self.bump();
                self.skip();
                continue;
            }
            break;
        }
        Ok(params)
    }

    fn parse_if(&mut self) -> Result<Statement> {
        self.expect_keyword("if")?;
        self.skip();
        self.expect(b'(')?;
        let cond = self.parse_expr()?;
        self.expect(b')')?;
        self.skip();
        let then_branch = Box::new(self.parse_statement()?);
        self.skip();
        let else_branch = if self.peek_keyword("else") {
            self.expect_keyword("else")?;
            self.skip();
            Some(Box::new(self.parse_statement()?))
        } else {
            None
        };
        Ok(Statement::If {
            cond,
            then_branch,
            else_branch,
        })
    }

    fn parse_for_any(&mut self) -> Result<Statement> {
        if self.peek_keyword("intersection_for") {
            self.expect_keyword("intersection_for")?;
        } else if self.peek_keyword("union_for") {
            self.expect_keyword("union_for")?;
        } else {
            self.expect_keyword("for")?;
        }
        self.skip();
        self.expect(b'(')?;
        let name = self.parse_ident()?;
        self.skip();
        self.expect(b'=')?;
        let range = self.parse_expr()?;
        self.expect(b')')?;
        self.skip();
        let body = Box::new(self.parse_statement()?);
        Ok(Statement::For { name, range, body })
    }

    fn parse_module_call(&mut self) -> Result<Statement> {
        let name = self.parse_ident()?;
        self.skip();
        self.expect(b'(')?;
        let args = self.parse_arg_list()?;
        self.expect(b')')?;
        self.skip();

        let children = if self.peek() == Some(b';') {
            self.bump();
            Vec::new()
        } else if self.peek() == Some(b'{') {
            match self.parse_block()? {
                Statement::Block(stmts) => stmts,
                other => vec![other],
            }
        } else if self.peek_is_ident()
            || self.peek_keyword("if")
            || self.peek_keyword("for")
            || self.peek() == Some(b'{')
        {
            vec![self.parse_statement()?]
        } else {
            Vec::new()
        };

        Ok(Statement::ModuleCall {
            name,
            args,
            children,
        })
    }

    fn parse_block(&mut self) -> Result<Statement> {
        self.expect(b'{')?;
        self.skip();
        let mut stmts = Vec::new();
        while self.peek() != Some(b'}') && !self.eof() {
            stmts.push(self.parse_statement()?);
            self.skip();
        }
        self.expect(b'}')?;
        Ok(Statement::Block(stmts))
    }

    fn parse_arg_list(&mut self) -> Result<Vec<Arg>> {
        self.skip();
        let mut args = Vec::new();
        if self.peek() == Some(b')') {
            return Ok(args);
        }
        loop {
            args.push(self.parse_arg()?);
            self.skip();
            if self.peek() == Some(b',') {
                self.bump();
                self.skip();
                continue;
            }
            break;
        }
        Ok(args)
    }

    fn parse_arg(&mut self) -> Result<Arg> {
        self.skip();
        let save = self.pos;
        if self.peek_is_ident() {
            let name = self.parse_ident()?;
            self.skip();
            if self.peek() == Some(b'=') {
                self.bump();
                let expr = self.parse_expr()?;
                return Ok(Arg::Named(name, expr));
            }
            self.pos = save;
        }
        Ok(Arg::Positional(self.parse_expr()?))
    }

    // ---- expressions ----
    // precedence: ternary < or < and < cmp < add < mul < unary < primary

    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_ternary()
    }

    fn parse_ternary(&mut self) -> Result<Expr> {
        let cond = self.parse_or()?;
        self.skip();
        if self.peek() == Some(b'?') {
            self.bump();
            let then_expr = self.parse_expr()?;
            self.skip();
            self.expect(b':')?;
            let else_expr = self.parse_expr()?;
            return Ok(Expr::Ternary {
                cond: Box::new(cond),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            });
        }
        Ok(cond)
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut left = self.parse_and()?;
        loop {
            self.skip();
            if self.peek_op("||") {
                self.pos += 2;
                let right = self.parse_and()?;
                left = Expr::Binary {
                    op: BinaryOp::Or,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_cmp()?;
        loop {
            self.skip();
            if self.peek_op("&&") {
                self.pos += 2;
                let right = self.parse_cmp()?;
                left = Expr::Binary {
                    op: BinaryOp::And,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_cmp(&mut self) -> Result<Expr> {
        let mut left = self.parse_add()?;
        loop {
            self.skip();
            let op = if self.peek_op("==") {
                self.pos += 2;
                Some(BinaryOp::Eq)
            } else if self.peek_op("!=") {
                self.pos += 2;
                Some(BinaryOp::Ne)
            } else if self.peek_op("<=") {
                self.pos += 2;
                Some(BinaryOp::Le)
            } else if self.peek_op(">=") {
                self.pos += 2;
                Some(BinaryOp::Ge)
            } else if self.peek() == Some(b'<') {
                self.bump();
                Some(BinaryOp::Lt)
            } else if self.peek() == Some(b'>') {
                self.bump();
                Some(BinaryOp::Gt)
            } else {
                None
            };
            if let Some(op) = op {
                let right = self.parse_add()?;
                left = Expr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_add(&mut self) -> Result<Expr> {
        let mut left = self.parse_mul()?;
        loop {
            self.skip();
            match self.peek() {
                Some(b'+') => {
                    self.bump();
                    let right = self.parse_mul()?;
                    left = Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                Some(b'-') => {
                    self.bump();
                    let right = self.parse_mul()?;
                    left = Expr::Binary {
                        op: BinaryOp::Sub,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<Expr> {
        let mut left = self.parse_pow()?;
        loop {
            self.skip();
            match self.peek() {
                Some(b'*') => {
                    self.bump();
                    let right = self.parse_pow()?;
                    left = Expr::Binary {
                        op: BinaryOp::Mul,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                Some(b'/') => {
                    self.bump();
                    let right = self.parse_pow()?;
                    left = Expr::Binary {
                        op: BinaryOp::Div,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                Some(b'%') => {
                    self.bump();
                    let right = self.parse_pow()?;
                    left = Expr::Binary {
                        op: BinaryOp::Mod,
                        left: Box::new(left),
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }
        Ok(left)
    }

    // exponentiation is right-associative: a^b^c = a^(b^c)
    fn parse_pow(&mut self) -> Result<Expr> {
        let left = self.parse_unary()?;
        self.skip();
        if self.peek() == Some(b'^') {
            self.bump();
            let right = self.parse_pow()?;
            return Ok(Expr::Binary {
                op: BinaryOp::Pow,
                left: Box::new(left),
                right: Box::new(right),
            });
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        self.skip();
        if self.peek() == Some(b'-') {
            self.bump();
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Neg,
                expr: Box::new(expr),
            });
        }
        if self.peek() == Some(b'!') {
            self.bump();
            let expr = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(expr),
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            self.skip();
            if self.peek() == Some(b'[') {
                self.bump();
                let index = self.parse_expr()?;
                self.expect(b']')?;
                expr = Expr::Index {
                    base: Box::new(expr),
                    index: Box::new(index),
                };
            } else if self.peek() == Some(b'.') {
                self.bump();
                let field = self.parse_ident()?;
                let idx = match field.as_str() {
                    "x" | "r" | "red" => 0.0,
                    "y" | "g" | "green" => 1.0,
                    "z" | "b" | "blue" => 2.0,
                    "a" | "alpha" => 3.0,
                    other => {
                        return Err(self.err(&format!("unknown field '.{}'", other)));
                    }
                };
                expr = Expr::Index {
                    base: Box::new(expr),
                    index: Box::new(Expr::Number(idx)),
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        self.skip();
        match self.peek() {
            Some(b'[') => self.parse_vector_or_range_or_comp(),
            Some(b'(') => {
                self.bump();
                let e = self.parse_expr()?;
                self.expect(b')')?;
                Ok(e)
            }
            Some(b'"') => Ok(Expr::String(self.parse_string()?)),
            Some(b'0'..=b'9') | Some(b'.') => Ok(Expr::Number(self.parse_number()?)),
            Some(b'a'..=b'z') | Some(b'A'..=b'Z') | Some(b'_') | Some(b'$') => {
                let name = self.parse_ident()?;
                if name == "true" {
                    return Ok(Expr::Bool(true));
                }
                if name == "false" {
                    return Ok(Expr::Bool(false));
                }
                self.skip();
                // function call in expression position
                if self.peek() == Some(b'(') {
                    self.bump();
                    let args = self.parse_arg_list()?;
                    self.expect(b')')?;
                    return Ok(Expr::Call { name, args });
                }
                Ok(Expr::Ident(name))
            }
            other => return Err(self.err(&format!(
                "unexpected {:?}",
                other.map(|b| b as char)
            ))),
        }
    }

    fn parse_vector_or_range_or_comp(&mut self) -> Result<Expr> {
        self.expect(b'[')?;
        self.skip();

        // list comprehension: [ for (x = r) expr ]
        if self.peek_keyword("for") {
            self.expect_keyword("for")?;
            self.skip();
            self.expect(b'(')?;
            let name = self.parse_ident()?;
            self.skip();
            self.expect(b'=')?;
            let range = self.parse_expr()?;
            self.expect(b')')?;
            self.skip();
            let body = self.parse_expr()?;
            self.skip();
            self.expect(b']')?;
            return Ok(Expr::ListComp {
                name,
                range: Box::new(range),
                body: Box::new(body),
            });
        }

        if self.peek() == Some(b']') {
            self.bump();
            return Ok(Expr::Vector(vec![]));
        }

        let first = self.parse_expr()?;
        self.skip();

        // range: [start : end] or [start : step : end]
        if self.peek() == Some(b':') {
            self.bump();
            let second = self.parse_expr()?;
            self.skip();
            if self.peek() == Some(b':') {
                self.bump();
                let end = self.parse_expr()?;
                self.skip();
                self.expect(b']')?;
                return Ok(Expr::Range {
                    start: Box::new(first),
                    step: Some(Box::new(second)),
                    end: Box::new(end),
                });
            }
            self.expect(b']')?;
            return Ok(Expr::Range {
                start: Box::new(first),
                step: None,
                end: Box::new(second),
            });
        }

        // vector
        let mut elems = vec![first];
        while self.peek() == Some(b',') {
            self.bump();
            self.skip();
            if self.peek() == Some(b']') {
                break;
            }
            elems.push(self.parse_expr()?);
            self.skip();
        }
        self.expect(b']')?;
        Ok(Expr::Vector(elems))
    }

    fn parse_string(&mut self) -> Result<String> {
        self.expect(b'"')?;
        let start = self.pos;
        while !self.eof() && self.peek() != Some(b'"') {
            if self.peek() == Some(b'\\') {
                self.bump();
            }
            self.bump();
        }
        let s = std::str::from_utf8(&self.src[start..self.pos])
            .unwrap()
            .to_string();
        self.expect(b'"')?;
        Ok(s)
    }

    fn parse_number(&mut self) -> Result<f64> {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9') | Some(b'.')) {
            self.bump();
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.bump();
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.bump();
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.bump();
            }
        }
        let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
        s.parse::<f64>()
            .map_err(|e| anyhow::anyhow!("invalid number '{}': {}", s, e))
    }

    fn parse_ident(&mut self) -> Result<String> {
        let start = self.pos;
        if !matches!(
            self.peek(),
            Some(b'a'..=b'z') | Some(b'A'..=b'Z') | Some(b'_') | Some(b'$')
        ) {
            return Err(self.err("expected identifier"));
        }
        self.bump();
        while matches!(
            self.peek(),
            Some(b'a'..=b'z')
                | Some(b'A'..=b'Z')
                | Some(b'0'..=b'9')
                | Some(b'_')
                | Some(b'$')
        ) {
            self.bump();
        }
        Ok(std::str::from_utf8(&self.src[start..self.pos])
            .unwrap()
            .to_string())
    }

    // ---- helpers ----

    fn skip(&mut self) {
        loop {
            while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                self.bump();
            }
            if self.peek() == Some(b'/') && self.peek_at(1) == Some(b'/') {
                self.pos += 2;
                while !self.eof() && self.peek() != Some(b'\n') {
                    self.bump();
                }
                continue;
            }
            if self.peek() == Some(b'/') && self.peek_at(1) == Some(b'*') {
                self.pos += 2;
                while !self.eof() {
                    if self.peek() == Some(b'*') && self.peek_at(1) == Some(b'/') {
                        self.pos += 2;
                        break;
                    }
                    self.bump();
                }
                continue;
            }
            break;
        }
    }

    fn expect(&mut self, b: u8) -> Result<()> {
        self.skip();
        if self.peek() == Some(b) {
            self.bump();
            Ok(())
        } else {
            Err(self.err(&format!(
                "expected '{}', found {:?}",
                b as char,
                self.peek().map(|c| c as char)
            )))
        }
    }

    fn peek_keyword(&self, kw: &str) -> bool {
        let bytes = kw.as_bytes();
        if self.pos + bytes.len() > self.src.len() {
            return false;
        }
        if &self.src[self.pos..self.pos + bytes.len()] != bytes {
            return false;
        }
        // word boundary
        let next = self.src.get(self.pos + bytes.len()).copied();
        !matches!(
            next,
            Some(b'a'..=b'z') | Some(b'A'..=b'Z') | Some(b'0'..=b'9') | Some(b'_') | Some(b'$')
        )
    }

    fn expect_keyword(&mut self, kw: &str) -> Result<()> {
        self.skip();
        if self.peek_keyword(kw) {
            self.pos += kw.len();
            Ok(())
        } else {
            bail!("expected keyword '{}' at {}", kw, self.pos)
        }
    }

    fn peek_op(&self, op: &str) -> bool {
        let bytes = op.as_bytes();
        self.pos + bytes.len() <= self.src.len()
            && &self.src[self.pos..self.pos + bytes.len()] == bytes
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn peek_at(&self, off: usize) -> Option<u8> {
        self.src.get(self.pos + off).copied()
    }

    fn peek_is_ident(&self) -> bool {
        matches!(
            self.peek(),
            Some(b'a'..=b'z') | Some(b'A'..=b'Z') | Some(b'_') | Some(b'$')
        )
    }

    fn bump(&mut self) {
        if self.pos < self.src.len() {
            self.pos += 1;
        }
    }

    fn eof(&self) -> bool {
        self.pos >= self.src.len()
    }
}
