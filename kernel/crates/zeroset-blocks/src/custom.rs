//! Tiny expression compiler for the Custom Field block.
//!
//! Grammar:
//! `program := ("def" ident "(" [ident ("," ident)*] ")" "=" body ";")*
//!            ("let" ident "=" value ";")* expr`
//! `body := expr | "{" ("let" ident "=" value ";")* expr "}"`
//! `expr := ident "(" [value ("," value)*] ")" | "$" ident`
//! `value := number | vec(number, number, number) | expr`
//! `expr` also supports `add/sub/mul/div`, `gt/ge/lt/le/eq`, and
//! `if(condition, then, else)`.
//!
//! `let` can bind a field expression, a number, or a vector. `def` creates a
//! parameterized field function; parameters are referenced as `$name`. Braced
//! function bodies can contain local `let` statements. External values (fields,
//! numbers, vectors, booleans) can be pre-bound as `$name` variables; the
//! Custom Field block binds optional inputs and dynamic UI ports; locals and
//! function parameters shadow them. `if` is lazy:
//! only the selected branch is parsed, so static recursion terminates without
//! building the unused branch.
//! `for(i, start, end, body)` expands body once per integer in `[start, end)`
//! and unions the resulting fields.

use std::collections::HashMap;
use std::fmt;

use glam::{Quat, Vec3};
use serde::Serialize;
use zeroset_core::{FieldGraph, NodeId};

use crate::unit::Unit;
use crate::value::ParamValue;

/// A ready-to-evaluate Custom Field expression shown in the editor preset menu.
#[derive(Clone, Debug, Serialize)]
pub struct CustomTemplate {
    pub name: &'static str,
    pub expression: &'static str,
    /// Optional parameter values applied together with the expression.
    pub params: Vec<(String, ParamValue)>,
}

fn template_number(name: &str, value: f32, unit: Unit) -> (String, ParamValue) {
    (name.to_string(), ParamValue::number(value, unit))
}

fn template_vector(name: &str, x: f32, y: f32, z: f32, unit: Unit) -> (String, ParamValue) {
    (name.to_string(), ParamValue::vector(x, y, z, unit))
}

fn template_boolean(name: &str, value: bool) -> (String, ParamValue) {
    (name.to_string(), ParamValue::Boolean(value))
}

/// Built-in expression templates. They only use `$scale`/`$offset`/
/// `$translation`/`$flag`, so every template can be evaluated without linking
/// any external field input. Templates may also preset the matching numeric,
/// vector, or boolean parameters.
pub fn custom_templates() -> Vec<CustomTemplate> {
    vec![
        CustomTemplate {
            name: "Sphere",
            expression: "sphere(0,0,0,10)",
            params: Vec::new(),
        },
        CustomTemplate {
            name: "Scaled sphere",
            expression: "sphere(0,0,0,mul(10,$scale))",
            params: vec![template_number("scale", 1.5, Unit::Unitless)],
        },
        CustomTemplate {
            name: "Offset shell",
            expression: "offset(sphere(0,0,0,10), $offset)",
            params: vec![template_number("offset", 2.0, Unit::Millimeter)],
        },
        CustomTemplate {
            name: "Translated sphere",
            expression: "sphere($translation, 10)",
            params: vec![template_vector(
                "translation",
                20.0,
                0.0,
                0.0,
                Unit::Millimeter,
            )],
        },
        CustomTemplate {
            name: "Smooth union",
            expression: "smooth_union(sphere(-6,0,0,7), sphere(6,0,0,7), 2)",
            params: Vec::new(),
        },
        CustomTemplate {
            name: "Subtract",
            expression: "subtract(sphere(0,0,0,10), box(0,8,0,8,2,8,1))",
            params: Vec::new(),
        },
        CustomTemplate {
            name: "Intersection",
            expression: "intersect(sphere(0,0,0,10), box(0,0,0,6,6,6,1))",
            params: Vec::new(),
        },
        CustomTemplate {
            name: "Torus",
            expression: "torus(0,0,0,0,1,0,10,3)",
            params: Vec::new(),
        },
        CustomTemplate {
            name: "Rotated box",
            expression: "transform(box(0,0,0,6,6,6,1), 0,0,0, 0,1,0, 45, 1)",
            params: Vec::new(),
        },
        CustomTemplate {
            name: "Boolean switch",
            expression: "if($flag, sphere(0,0,0,10), box(0,0,0,6,6,6,1))",
            params: vec![template_boolean("flag", true)],
        },
    ]
}

/// Structured location for a Custom Field parse error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomDiagnostic {
    pub message: String,
    pub line: u32,
    pub column: u32,
    /// Source line with its `N | ` gutter.
    pub snippet: String,
    /// Caret line, including leading spaces.
    pub caret: String,
}

impl CustomDiagnostic {
    /// Splits the human-readable parser error back into source coordinates.
    pub fn from_message(text: &str) -> Option<Self> {
        let (message, tail) = text.rsplit_once(" at line ")?;
        let (line, tail) = tail.split_once(", column ")?;
        let (column, tail) = tail.split_once('\n')?;
        let (snippet, caret) = tail.split_once('\n')?;
        if !caret.trim_end().ends_with('^') {
            return None;
        }
        Some(Self {
            message: message.to_string(),
            line: line.parse().ok()?,
            column: column.parse().ok()?,
            snippet: snippet.to_string(),
            caret: caret.to_string(),
        })
    }
}

impl fmt::Display for CustomDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at line {}, column {}\n{}\n{}",
            self.message, self.line, self.column, self.snippet, self.caret
        )
    }
}

#[derive(Clone, Copy)]
enum Arg {
    Number(f32),
    Field(NodeId),
    Vector(Vec3),
    Bool(bool),
}

/// Values that can be pre-bound as `$name` variables before parsing.
#[derive(Clone, Copy, Debug)]
pub enum Binding {
    Number(f32),
    Field(NodeId),
    Vector(Vec3),
    Boolean(bool),
}

#[derive(Clone)]
struct FunctionDef {
    params: Vec<String>,
    body: String,
}

/// Compiles `source` with pre-bound variables (for example the optional
/// inputs of the Custom Field block). Locals and `def` parameters shadow
/// these bindings.
pub fn parse_with_bindings(
    graph: &mut FieldGraph,
    source: &str,
    bindings: &[(String, Binding)],
) -> Result<NodeId, String> {
    let mut env = HashMap::new();
    for (name, binding) in bindings {
        let arg = match binding {
            Binding::Number(value) => Arg::Number(*value),
            Binding::Field(node) => Arg::Field(*node),
            Binding::Vector(value) => Arg::Vector(*value),
            Binding::Boolean(value) => Arg::Bool(*value),
        };
        env.insert(name.clone(), arg);
    }
    let mut parser = Parser {
        source,
        bytes: source.as_bytes(),
        position: 0,
        graph,
        env,
        functions: HashMap::new(),
        depth: 0,
    };
    parser.parse_program()
}

fn parse_scoped(
    graph: &mut FieldGraph,
    source: &str,
    env: HashMap<String, Arg>,
    functions: HashMap<String, FunctionDef>,
    depth: usize,
) -> Result<Arg, String> {
    let mut parser = Parser {
        source,
        bytes: source.as_bytes(),
        position: 0,
        graph,
        env,
        functions,
        depth,
    };
    parser.parse_block().map(Arg::Field)
}

fn parse_scoped_value(
    graph: &mut FieldGraph,
    source: &str,
    env: HashMap<String, Arg>,
    functions: HashMap<String, FunctionDef>,
    depth: usize,
) -> Result<Arg, String> {
    let mut parser = Parser {
        source,
        bytes: source.as_bytes(),
        position: 0,
        graph,
        env,
        functions,
        depth,
    };
    parser.parse_block_value()
}

struct Parser<'a> {
    source: &'a str,
    bytes: &'a [u8],
    position: usize,
    graph: &'a mut FieldGraph,
    env: HashMap<String, Arg>,
    functions: HashMap<String, FunctionDef>,
    depth: usize,
}

impl Parser<'_> {
    fn parse_program(&mut self) -> Result<NodeId, String> {
        loop {
            self.skip_whitespace();
            if !self.starts_keyword("def") {
                break;
            }
            self.parse_definition()?;
        }
        self.parse_block()
    }

    fn parse_block(&mut self) -> Result<NodeId, String> {
        match self.parse_block_value()? {
            Arg::Field(node) => Ok(node),
            Arg::Number(_) | Arg::Vector(_) | Arg::Bool(_) => {
                Err(self.error("expected a field expression"))
            }
        }
    }

    fn parse_block_value(&mut self) -> Result<Arg, String> {
        loop {
            self.skip_whitespace();
            if !self.starts_keyword("let") {
                break;
            }
            self.position += 3;
            let name = self.parse_identifier()?;
            self.expect(b'=')?;
            let value = self.parse_value()?;
            self.env.insert(name, value);
            self.expect(b';')?;
        }
        let node = self.parse_value()?;
        self.skip_whitespace();
        if self.peek() == Some(b';') {
            self.position += 1;
            self.skip_whitespace();
        }
        if self.position != self.bytes.len() {
            return Err(self.error("trailing input"));
        }
        Ok(node)
    }

    fn parse_definition(&mut self) -> Result<(), String> {
        self.position += 3;
        let name = self.parse_identifier()?.to_ascii_lowercase();
        self.expect(b'(')?;
        let mut params = Vec::new();
        self.skip_whitespace();
        if self.peek() != Some(b')') {
            loop {
                params.push(self.parse_identifier()?);
                self.skip_whitespace();
                if self.peek() == Some(b',') {
                    self.position += 1;
                } else {
                    break;
                }
            }
        }
        self.expect(b')')?;
        self.expect(b'=')?;
        self.skip_whitespace();
        let body = if self.peek() == Some(b'{') {
            self.position += 1;
            let start = self.position;
            let mut depth = 1i32;
            let mut end = None;
            while self.position < self.bytes.len() {
                match self.peek() {
                    Some(b'{') => depth += 1,
                    Some(b'}') => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(self.position);
                            break;
                        }
                    }
                    _ => {}
                }
                self.position += 1;
            }
            let end = end.ok_or_else(|| self.error("unterminated function body `{`"))?;
            let body = self.source[start..end].to_string();
            self.position = end + 1;
            self.skip_whitespace();
            if self.peek() == Some(b';') {
                self.position += 1;
            }
            body
        } else {
            let start = self.position;
            let mut depth = 0i32;
            let mut end = None;
            while self.position < self.bytes.len() {
                match self.peek() {
                    Some(b'(') => depth += 1,
                    Some(b')') => depth -= 1,
                    Some(b';') if depth == 0 => {
                        end = Some(self.position);
                        break;
                    }
                    _ => {}
                }
                self.position += 1;
            }
            let end = end.ok_or_else(|| self.error("function body needs a trailing `;`"))?;
            let body = self.source[start..end].to_string();
            self.position = end + 1;
            body
        };
        if body.trim().is_empty() {
            return Err(self.error("function body is empty"));
        }
        self.functions.insert(name, FunctionDef { params, body });
        Ok(())
    }

    fn starts_keyword(&self, keyword: &str) -> bool {
        let end = self.position + keyword.len();
        if end > self.bytes.len() || &self.source[self.position..end] != keyword {
            return false;
        }
        self.bytes
            .get(end)
            .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
    }

    fn parse_value(&mut self) -> Result<Arg, String> {
        self.skip_whitespace();
        if self.position >= self.bytes.len() {
            return Err(self.error("expected value"));
        }
        if self.depth > 64 {
            return Err(self.error("expression nesting is too deep"));
        }
        self.depth += 1;
        let result = if self.peek() == Some(b'$') {
            self.position += 1;
            let name = self.parse_identifier()?;
            self.env
                .get(&name)
                .copied()
                .ok_or_else(|| self.error(&format!("unknown variable `${name}`")))
        } else if self.peek() == Some(b'-')
            || self.peek().is_some_and(|b| b.is_ascii_digit() || b == b'.')
        {
            Ok(Arg::Number(self.parse_number()?))
        } else {
            let name = self.parse_identifier()?;
            self.expect(b'(')?;
            if name.eq_ignore_ascii_case("if") {
                let value = self.parse_if()?;
                self.depth -= 1;
                return Ok(value);
            }
            if name.eq_ignore_ascii_case("for") {
                let value = self.parse_for()?;
                self.depth -= 1;
                return Ok(value);
            }
            let mut args = Vec::new();
            self.skip_whitespace();
            if self.peek() != Some(b')') {
                loop {
                    args.push(self.parse_value()?);
                    self.skip_whitespace();
                    if self.peek() == Some(b',') {
                        self.position += 1;
                    } else {
                        break;
                    }
                }
            }
            self.expect(b')')?;
            let key = name.to_ascii_lowercase();
            if let Some(function) = self.functions.get(&key).cloned() {
                if args.len() != function.params.len() {
                    return Err(self.error(&format!(
                        "{name} expects {} arguments, got {}",
                        function.params.len(),
                        args.len()
                    )));
                }
                let mut env = self.env.clone();
                for (param, value) in function.params.iter().zip(args.iter()) {
                    env.insert(param.clone(), *value);
                }
                return parse_scoped(
                    self.graph,
                    &function.body,
                    env,
                    self.functions.clone(),
                    self.depth,
                );
            }
            self.build_call(&name, &args)
        };
        self.depth -= 1;
        result
    }

    fn parse_if(&mut self) -> Result<Arg, String> {
        let args = self.split_call_args()?;
        if args.len() != 3 {
            return Err(self.error("if expects condition, then, else"));
        }
        let condition = parse_scoped_value(
            self.graph,
            &args[0],
            self.env.clone(),
            self.functions.clone(),
            self.depth,
        )?;
        let branch = match condition {
            Arg::Bool(true) => 1,
            Arg::Bool(false) => 2,
            _ => return Err(self.error("if condition must be a boolean")),
        };
        parse_scoped_value(
            self.graph,
            &args[branch],
            self.env.clone(),
            self.functions.clone(),
            self.depth,
        )
    }

    fn parse_for(&mut self) -> Result<Arg, String> {
        let args = self.split_call_args()?;
        if args.len() != 4 {
            return Err(self.error("for expects variable, start, end, body"));
        }
        let variable = args[0].trim().trim_start_matches('$').to_string();
        if variable.is_empty()
            || !variable
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            return Err(self.error("for variable must be an identifier"));
        }
        let start = match parse_scoped_value(
            self.graph,
            &args[1],
            self.env.clone(),
            self.functions.clone(),
            self.depth,
        )? {
            Arg::Number(value) => value,
            _ => return Err(self.error("for start must be a number")),
        };
        let end = match parse_scoped_value(
            self.graph,
            &args[2],
            self.env.clone(),
            self.functions.clone(),
            self.depth,
        )? {
            Arg::Number(value) => value,
            _ => return Err(self.error("for end must be a number")),
        };
        let count = (end - start).round();
        if !count.is_finite() || count < 0.0 {
            return Err(self.error("for range must be finite and non-negative"));
        }
        if count > 256.0 {
            return Err(self.error("for range is limited to 256 iterations"));
        }
        let mut root = None;
        for index in 0..count as i32 {
            let mut env = self.env.clone();
            env.insert(variable.clone(), Arg::Number(start + index as f32));
            let value = parse_scoped_value(
                self.graph,
                &args[3],
                env,
                self.functions.clone(),
                self.depth,
            )?;
            let Arg::Field(node) = value else {
                return Err(self.error("for body must be a field expression"));
            };
            root = Some(match root {
                None => node,
                Some(parent) => self.graph.union(parent, node),
            });
        }
        let node = root.ok_or_else(|| self.error("for loop produced no iterations"))?;
        Ok(Arg::Field(node))
    }

    fn split_call_args(&mut self) -> Result<Vec<String>, String> {
        let mut args = Vec::new();
        let mut start = self.position;
        let mut depth = 0i32;
        while self.position < self.bytes.len() {
            match self.peek() {
                Some(b'(') => depth += 1,
                Some(b')') => {
                    if depth == 0 {
                        if self.position > start || !args.is_empty() {
                            args.push(self.source[start..self.position].to_string());
                        }
                        self.position += 1;
                        return Ok(args);
                    }
                    depth -= 1;
                }
                Some(b',') if depth == 0 => {
                    args.push(self.source[start..self.position].to_string());
                    start = self.position + 1;
                }
                _ => {}
            }
            self.position += 1;
        }
        Err(self.error("unterminated call arguments"))
    }

    fn build_call(&mut self, name: &str, args: &[Arg]) -> Result<Arg, String> {
        let name = name.to_ascii_lowercase();
        match name.as_str() {
            "add" | "sub" | "mul" | "div" => {
                let n = self.numbers(args, 2, &name)?;
                let value = match name.as_str() {
                    "add" => n[0] + n[1],
                    "sub" => n[0] - n[1],
                    "mul" => n[0] * n[1],
                    _ => {
                        if n[1].abs() <= f32::EPSILON {
                            return Err(self.error("division by zero"));
                        }
                        n[0] / n[1]
                    }
                };
                Ok(Arg::Number(value))
            }
            "gt" | "ge" | "lt" | "le" | "eq" => {
                let n = self.numbers(args, 2, &name)?;
                let value = match name.as_str() {
                    "gt" => n[0] > n[1],
                    "ge" => n[0] >= n[1],
                    "lt" => n[0] < n[1],
                    "le" => n[0] <= n[1],
                    _ => (n[0] - n[1]).abs() <= 1e-6,
                };
                Ok(Arg::Bool(value))
            }
            "and" | "or" => {
                if args.len() != 2 {
                    return Err(self.error(&format!("{name} expects two booleans")));
                }
                let (Arg::Bool(left), Arg::Bool(right)) = (args[0], args[1]) else {
                    return Err(self.error(&format!("{name} expects two booleans")));
                };
                Ok(Arg::Bool(if name == "and" {
                    left && right
                } else {
                    left || right
                }))
            }
            "not" => {
                if args.len() != 1 {
                    return Err(self.error("not expects one boolean"));
                }
                match args[0] {
                    Arg::Bool(value) => Ok(Arg::Bool(!value)),
                    _ => Err(self.error("not expects a boolean")),
                }
            }
            "if" => {
                if args.len() != 3 {
                    return Err(self.error("if expects condition, then, else"));
                }
                match args[0] {
                    Arg::Bool(condition) => Ok(if condition { args[1] } else { args[2] }),
                    _ => Err(self.error("if condition must be a boolean")),
                }
            }
            "vec" => {
                let n = self.numbers(args, 3, &name)?;
                Ok(Arg::Vector(Vec3::new(n[0], n[1], n[2])))
            }
            "sphere" => {
                if args.len() == 2 {
                    return Ok(Arg::Field(self.graph.sphere(
                        self.vector_arg(args[0], &name)?,
                        self.number_arg(args[1], &name)?,
                    )));
                }
                let n = self.numbers(args, 4, &name)?;
                Ok(Arg::Field(
                    self.graph.sphere(Vec3::new(n[0], n[1], n[2]), n[3]),
                ))
            }
            "box" => {
                if args.len() == 3 {
                    return Ok(Arg::Field(self.graph.rounded_box(
                        self.vector_arg(args[0], &name)?,
                        self.vector_arg(args[1], &name)?,
                        self.number_arg(args[2], &name)?,
                    )));
                }
                let n = self.numbers(args, 7, &name)?;
                Ok(Arg::Field(self.graph.rounded_box(
                    Vec3::new(n[0], n[1], n[2]),
                    Vec3::new(n[3], n[4], n[5]),
                    n[6],
                )))
            }
            "cylinder" => {
                if args.len() == 4 {
                    return Ok(Arg::Field(self.graph.cylinder(
                        self.vector_arg(args[0], &name)?,
                        self.vector_arg(args[1], &name)?,
                        self.number_arg(args[2], &name)?,
                        self.number_arg(args[3], &name)?,
                    )));
                }
                let n = self.numbers(args, 8, &name)?;
                Ok(Arg::Field(self.graph.cylinder(
                    Vec3::new(n[0], n[1], n[2]),
                    Vec3::new(n[3], n[4], n[5]),
                    n[6],
                    n[7],
                )))
            }
            "capsule" => {
                if args.len() == 3 {
                    return Ok(Arg::Field(self.graph.capsule(
                        self.vector_arg(args[0], &name)?,
                        self.vector_arg(args[1], &name)?,
                        self.number_arg(args[2], &name)?,
                    )));
                }
                let n = self.numbers(args, 7, &name)?;
                Ok(Arg::Field(self.graph.capsule(
                    Vec3::new(n[0], n[1], n[2]),
                    Vec3::new(n[3], n[4], n[5]),
                    n[6],
                )))
            }
            "torus" => {
                if args.len() == 4 {
                    return Ok(Arg::Field(self.graph.torus(
                        self.vector_arg(args[0], &name)?,
                        self.vector_arg(args[1], &name)?,
                        self.number_arg(args[2], &name)?,
                        self.number_arg(args[3], &name)?,
                    )));
                }
                let n = self.numbers(args, 8, &name)?;
                Ok(Arg::Field(self.graph.torus(
                    Vec3::new(n[0], n[1], n[2]),
                    Vec3::new(n[3], n[4], n[5]),
                    n[6],
                    n[7],
                )))
            }
            "union" | "intersect" | "subtract" => {
                let [a, b] = self.fields(args, 2, &name)?;
                Ok(Arg::Field(match name.as_str() {
                    "union" => self.graph.union(a, b),
                    "intersect" => self.graph.intersect(a, b),
                    _ => self.graph.subtract(a, b),
                }))
            }
            "smooth_union" | "smooth_intersect" | "smooth_subtract" => {
                let [a, b] = self.fields(args, 2, &name)?;
                let n = self.numbers(&args[2..], 1, &name)?;
                Ok(Arg::Field(match name.as_str() {
                    "smooth_union" => self.graph.smooth_union(a, b, n[0]),
                    "smooth_intersect" => self.graph.smooth_intersect(a, b, n[0]),
                    _ => self.graph.smooth_subtract(a, b, n[0]),
                }))
            }
            "offset" => {
                let [field] = self.fields(args, 1, &name)?;
                let n = self.numbers(&args[1..], 1, &name)?;
                Ok(Arg::Field(self.graph.offset(field, n[0])))
            }
            "transform" => {
                let [field] = self.fields(args, 1, &name)?;
                let n = self.numbers(&args[1..], 8, &name)?;
                let axis = Vec3::new(n[3], n[4], n[5]);
                if axis.length_squared() < 1e-12 {
                    return Err(self.error("transform axis must be non-zero"));
                }
                let transform = zeroset_core::Transform::new(
                    Vec3::new(n[0], n[1], n[2]),
                    Quat::from_axis_angle(axis.normalize(), n[6].to_radians()),
                    n[7],
                )
                .map_err(|error| error.to_string())?;
                Ok(Arg::Field(self.graph.transform(field, transform)))
            }
            _ => Err(self.error(&format!("unknown function `{name}`"))),
        }
    }

    fn number_arg(&self, arg: Arg, name: &str) -> Result<f32, String> {
        match arg {
            Arg::Number(value) => Ok(value),
            _ => Err(self.error(&format!("{name} expects a number"))),
        }
    }

    fn vector_arg(&self, arg: Arg, name: &str) -> Result<Vec3, String> {
        match arg {
            Arg::Vector(value) => Ok(value),
            _ => Err(self.error(&format!("{name} expects a vector"))),
        }
    }

    fn numbers(&self, args: &[Arg], count: usize, name: &str) -> Result<Vec<f32>, String> {
        if args.len() != count {
            return Err(self.error(&format!(
                "{name} expects {count} arguments, got {}",
                args.len()
            )));
        }
        let mut numbers = Vec::with_capacity(count);
        for arg in args {
            match arg {
                Arg::Number(value) => numbers.push(*value),
                Arg::Field(_) | Arg::Vector(_) | Arg::Bool(_) => {
                    return Err(self.error(&format!("{name} expects numbers")));
                }
            }
        }
        Ok(numbers)
    }

    fn fields<const N: usize>(
        &self,
        args: &[Arg],
        count: usize,
        name: &str,
    ) -> Result<[NodeId; N], String> {
        if args.len() < count {
            return Err(self.error(&format!(
                "{name} expects {count} fields, got {}",
                args.len()
            )));
        }
        let mut fields = [NodeId(0); N];
        for (index, slot) in fields.iter_mut().enumerate() {
            match args.get(index) {
                Some(Arg::Field(node)) => *slot = *node,
                _ => return Err(self.error(&format!("{name} expects fields"))),
            }
        }
        Ok(fields)
    }

    fn parse_identifier(&mut self) -> Result<String, String> {
        self.skip_whitespace();
        let start = self.position;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_alphanumeric() || byte == b'_' {
                self.position += 1;
            } else {
                break;
            }
        }
        if self.position == start {
            return Err(self.error("expected identifier"));
        }
        Ok(self.source[start..self.position].to_string())
    }

    fn parse_number(&mut self) -> Result<f32, String> {
        self.skip_whitespace();
        let start = self.position;
        if self.peek() == Some(b'-') {
            self.position += 1;
        }
        let mut has_digit = false;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_digit()
                || byte == b'.'
                || byte == b'e'
                || byte == b'E'
                || byte == b'+'
                || byte == b'-'
            {
                if byte.is_ascii_digit() {
                    has_digit = true;
                }
                self.position += 1;
            } else {
                break;
            }
        }
        if !has_digit {
            return Err(self.error("invalid number"));
        }
        self.source[start..self.position]
            .parse::<f32>()
            .map_err(|_| self.error("invalid number"))
    }

    fn expect(&mut self, expected: u8) -> Result<(), String> {
        self.skip_whitespace();
        if self.peek() == Some(expected) {
            self.position += 1;
            Ok(())
        } else {
            Err(self.error(&format!("expected `{}`", expected as char)))
        }
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.position += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn error(&self, message: &str) -> String {
        let offset = self.position.min(self.source.len());
        let prefix = &self.source[..offset];
        let line = prefix.matches('\n').count() + 1;
        let line_start = prefix.rfind('\n').map(|index| index + 1).unwrap_or(0);
        let column = self.source[line_start..offset].chars().count() + 1;
        let line_text = self.source.lines().nth(line - 1).unwrap_or("");
        let gutter = format!("{line} | ");
        format!(
            "{message} at line {line}, column {column}\n{gutter}{line_text}\n{}^",
            " ".repeat(gutter.len() + column - 1)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_parse_with_default_bindings() {
        let bindings = [
            ("scale".to_string(), Binding::Number(1.0)),
            ("offset".to_string(), Binding::Number(0.0)),
            ("translation".to_string(), Binding::Vector(Vec3::ZERO)),
            ("flag".to_string(), Binding::Boolean(true)),
        ];
        assert!(custom_templates().len() >= 8);
        for template in custom_templates() {
            let mut graph = FieldGraph::new();
            let node = parse_with_bindings(&mut graph, template.expression, &bindings)
                .unwrap_or_else(|error| panic!("{}: {error}", template.name));
            assert!(
                graph.bounds_of(node).is_finite(),
                "{} produced non-finite bounds",
                template.name
            );
        }
        let templates = custom_templates();
        let offset = templates
            .iter()
            .find(|template| template.name == "Offset shell")
            .expect("offset template");
        assert!(offset.params.iter().any(|(name, value)| {
            name == "offset"
                && matches!(
                    value,
                    ParamValue::Number(quantity)
                        if quantity.unit == Unit::Millimeter && (quantity.value - 2.0).abs() < 1e-6
                )
        }));
    }
}
