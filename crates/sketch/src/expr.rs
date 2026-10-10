//! Expression engine for parametric dimensions.
//!
//! Grammar (recursive descent, `=` prefix optional):
//! ```text
//! expr    := term (('+' | '-') term)*
//! term    := unary (('*' | '/') unary)*
//! unary   := ('-' | '+') unary | power
//! power   := primary ('^' unary)?            // right-assoc, binds tighter
//!                                           // than unary minus: -2^2 = -4
//! primary := number | ident | func '(' args ')' | '(' expr ')'
//! func    := sin | cos | tan | sqrt | abs | min | max | floor | ceil
//! ```
//! Implicit units: mm for lengths, degrees for angles; sin/cos/tan take
//! degrees. Identifiers reference sketch parameters (d1, d2, …).

use std::fmt;

/// Errors of expression parsing/evaluation, with user-facing messages.
#[derive(Debug, Clone, PartialEq)]
pub enum ExprError {
    /// Malformed input; the message names the offending token/position.
    UnexpectedToken(String),
    /// Reference to a parameter that does not exist in the sketch.
    UnknownParameter(String),
    DivisionByZero,
    /// Circular parameter reference; the vec is the cycle path
    /// (e.g. ["d3", "d5", "d3"]).
    CircularReference(Vec<String>),
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExprError::UnexpectedToken(msg) => write!(f, "invalid expression: {msg}"),
            ExprError::UnknownParameter(name) => write!(f, "unknown parameter '{name}'"),
            ExprError::DivisionByZero => write!(f, "division by zero"),
            ExprError::CircularReference(cycle) => {
                write!(f, "circular reference: {}", cycle.join(" → "))
            }
        }
    }
}

impl std::error::Error for ExprError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Func {
    Sin,
    Cos,
    Tan,
    Sqrt,
    Abs,
    Min,
    Max,
    Floor,
    Ceil,
}

impl Func {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "sin" => Func::Sin,
            "cos" => Func::Cos,
            "tan" => Func::Tan,
            "sqrt" => Func::Sqrt,
            "abs" => Func::Abs,
            "min" => Func::Min,
            "max" => Func::Max,
            "floor" => Func::Floor,
            "ceil" => Func::Ceil,
            _ => return None,
        })
    }

    fn arity(self) -> usize {
        match self {
            Func::Min | Func::Max => 2,
            _ => 1,
        }
    }

    fn apply(self, args: &[f64]) -> Result<f64, ExprError> {
        const DEG: f64 = std::f64::consts::PI / 180.0;
        Ok(match self {
            Func::Sin => (args[0] * DEG).sin(),
            Func::Cos => (args[0] * DEG).cos(),
            Func::Tan => (args[0] * DEG).tan(),
            Func::Sqrt => {
                if args[0] < 0.0 {
                    return Err(ExprError::UnexpectedToken(
                        "sqrt of a negative number".to_string(),
                    ));
                }
                args[0].sqrt()
            }
            Func::Abs => args[0].abs(),
            Func::Min => args[0].min(args[1]),
            Func::Max => args[0].max(args[1]),
            Func::Floor => args[0].floor(),
            Func::Ceil => args[0].ceil(),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ast {
    Num(f64),
    Ident(String),
    UnaryNeg(Box<Ast>),
    Bin(Op, Box<Ast>, Box<Ast>),
    Call(Func, Vec<Ast>),
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
    LParen,
    RParen,
    Comma,
    End,
}

fn tokenize(input: &str) -> Result<Vec<Tok>, ExprError> {
    let mut toks = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' | '\n' => {
                chars.next();
            }
            '0'..='9' | '.' => {
                let mut text = String::new();
                let mut seen_dot = false;
                let mut seen_digit = false;
                while let Some(&c) = chars.peek() {
                    match c {
                        '0'..='9' => {
                            seen_digit = true;
                            text.push(c);
                            chars.next();
                        }
                        '.' if !seen_dot => {
                            seen_dot = true;
                            text.push(c);
                            chars.next();
                        }
                        _ => break,
                    }
                }
                if !seen_digit {
                    return Err(ExprError::UnexpectedToken(format!("'{c}'")));
                }
                let value: f64 = text
                    .parse()
                    .map_err(|_| ExprError::UnexpectedToken(format!("'{text}'")))?;
                toks.push(Tok::Num(value));
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let mut name = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                toks.push(Tok::Ident(name));
            }
            '+' | '-' | '*' | '/' | '^' => {
                toks.push(Tok::Op(c));
                chars.next();
            }
            '(' => {
                toks.push(Tok::LParen);
                chars.next();
            }
            ')' => {
                toks.push(Tok::RParen);
                chars.next();
            }
            ',' => {
                toks.push(Tok::Comma);
                chars.next();
            }
            other => return Err(ExprError::UnexpectedToken(format!("'{other}'"))),
        }
    }
    toks.push(Tok::End);
    Ok(toks)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos]
    }

    fn next(&mut self) -> Tok {
        let tok = self.toks[self.pos].clone();
        self.pos += 1;
        tok
    }

    fn expect(&mut self, want: &Tok) -> Result<(), ExprError> {
        if self.peek() == want {
            self.next();
            Ok(())
        } else {
            Err(ExprError::UnexpectedToken(format!(
                "expected {:?}, found {:?}",
                want,
                self.peek()
            )))
        }
    }

    fn parse_expr(&mut self) -> Result<Ast, ExprError> {
        let mut left = self.parse_term()?;
        loop {
            match self.peek() {
                Tok::Op('+') => {
                    self.next();
                    let right = self.parse_term()?;
                    left = Ast::Bin(Op::Add, Box::new(left), Box::new(right));
                }
                Tok::Op('-') => {
                    self.next();
                    let right = self.parse_term()?;
                    left = Ast::Bin(Op::Sub, Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<Ast, ExprError> {
        let mut left = self.parse_unary()?;
        loop {
            match self.peek() {
                Tok::Op('*') => {
                    self.next();
                    let right = self.parse_unary()?;
                    left = Ast::Bin(Op::Mul, Box::new(left), Box::new(right));
                }
                Tok::Op('/') => {
                    self.next();
                    let right = self.parse_unary()?;
                    left = Ast::Bin(Op::Div, Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Ast, ExprError> {
        if *self.peek() == Tok::Op('+') {
            self.next();
            return self.parse_unary();
        }
        if *self.peek() == Tok::Op('-') {
            self.next();
            return Ok(Ast::UnaryNeg(Box::new(self.parse_unary()?)));
        }
        self.parse_power()
    }

    fn parse_power(&mut self) -> Result<Ast, ExprError> {
        let base = self.parse_primary()?;
        if *self.peek() == Tok::Op('^') {
            self.next();

            let exp = self.parse_unary()?;
            return Ok(Ast::Bin(Op::Pow, Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn parse_primary(&mut self) -> Result<Ast, ExprError> {
        match self.next() {
            Tok::Num(v) => Ok(Ast::Num(v)),
            Tok::Ident(name) => {
                if *self.peek() == Tok::LParen {
                    self.next();
                    let Some(func) = Func::from_name(&name) else {
                        return Err(ExprError::UnexpectedToken(format!(
                            "unknown function '{name}'"
                        )));
                    };
                    let mut args = Vec::new();
                    if *self.peek() != Tok::RParen {
                        loop {
                            args.push(self.parse_expr()?);
                            if *self.peek() == Tok::Comma {
                                self.next();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(&Tok::RParen)?;
                    if args.len() != func.arity() {
                        return Err(ExprError::UnexpectedToken(format!(
                            "'{name}' takes {} argument(s)",
                            func.arity()
                        )));
                    }
                    Ok(Ast::Call(func, args))
                } else {
                    Ok(Ast::Ident(name))
                }
            }
            Tok::LParen => {
                let inner = self.parse_expr()?;
                self.expect(&Tok::RParen)?;
                Ok(inner)
            }
            other => Err(ExprError::UnexpectedToken(format!("{other:?}"))),
        }
    }
}

/// Parse an expression (leading `=` optional) to an AST.
pub fn parse(input: &str) -> Result<Ast, ExprError> {
    let trimmed = input
        .trim()
        .strip_prefix('=')
        .unwrap_or(input.trim())
        .trim();
    if trimmed.is_empty() {
        return Err(ExprError::UnexpectedToken("empty expression".to_string()));
    }
    let mut parser = Parser {
        toks: tokenize(trimmed)?,
        pos: 0,
    };
    let ast = parser.parse_expr()?;
    if *parser.peek() != Tok::End {
        return Err(ExprError::UnexpectedToken(format!(
            "trailing input {:?}",
            parser.peek()
        )));
    }
    Ok(ast)
}

/// Evaluate a parsed expression with a parameter resolver.
pub fn eval(
    ast: &Ast,
    resolver: &mut impl FnMut(&str) -> Result<f64, ExprError>,
) -> Result<f64, ExprError> {
    match ast {
        Ast::Num(v) => Ok(*v),
        Ast::Ident(name) => resolver(name),
        Ast::UnaryNeg(inner) => Ok(-eval(inner, resolver)?),
        Ast::Bin(op, a, b) => {
            let (x, y) = (eval(a, resolver)?, eval(b, resolver)?);
            Ok(match op {
                Op::Add => x + y,
                Op::Sub => x - y,
                Op::Mul => x * y,
                Op::Div => {
                    if y == 0.0 {
                        return Err(ExprError::DivisionByZero);
                    }
                    x / y
                }
                Op::Pow => x.powf(y),
            })
        }
        Ast::Call(func, args) => {
            let values: Vec<f64> = args
                .iter()
                .map(|a| eval(a, resolver))
                .collect::<Result<_, _>>()?;
            func.apply(&values)
        }
    }
}

/// Evaluate an expression string against a resolver (parse + eval).
pub fn eval_expression(
    input: &str,
    resolver: &mut impl FnMut(&str) -> Result<f64, ExprError>,
) -> Result<f64, ExprError> {
    eval(&parse(input)?, resolver)
}

/// Identifiers referenced by an expression (parameter dependencies).
pub fn referenced_idents(ast: &Ast) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(ast: &Ast, out: &mut Vec<String>) {
        match ast {
            Ast::Num(_) => {}
            Ast::Ident(name) => {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Ast::UnaryNeg(inner) => walk(inner, out),
            Ast::Bin(_, a, b) => {
                walk(a, out);
                walk(b, out);
            }
            Ast::Call(_, args) => {
                for a in args {
                    walk(a, out);
                }
            }
        }
    }
    walk(ast, &mut out);
    out
}

/// Rename an identifier throughout an AST (parameter rename support).
pub fn rename_ident(ast: &mut Ast, from: &str, to: &str) {
    match ast {
        Ast::Num(_) => {}
        Ast::Ident(name) => {
            if name == from {
                *name = to.to_string();
            }
        }
        Ast::UnaryNeg(inner) => rename_ident(inner, from, to),
        Ast::Bin(_, a, b) => {
            rename_ident(a, from, to);
            rename_ident(b, from, to);
        }
        Ast::Call(_, args) => {
            for a in args {
                rename_ident(a, from, to);
            }
        }
    }
}

/// Serialize an AST back to an expression string (rename roundtrip).
pub fn to_string(ast: &Ast) -> String {
    match ast {
        Ast::Num(v) => {
            let s = format!("{v}");
            s
        }
        Ast::Ident(name) => name.clone(),
        Ast::UnaryNeg(inner) => format!("-{}", to_string(inner)),
        Ast::Bin(op, a, b) => {
            let sym = match op {
                Op::Add => "+",
                Op::Sub => "-",
                Op::Mul => "*",
                Op::Div => "/",
                Op::Pow => "^",
            };
            format!("({} {} {})", to_string(a), sym, to_string(b))
        }
        Ast::Call(func, args) => {
            let name = match func {
                Func::Sin => "sin",
                Func::Cos => "cos",
                Func::Tan => "tan",
                Func::Sqrt => "sqrt",
                Func::Abs => "abs",
                Func::Min => "min",
                Func::Max => "max",
                Func::Floor => "floor",
                Func::Ceil => "ceil",
            };
            let args = args.iter().map(to_string).collect::<Vec<_>>().join(", ");
            format!("{name}({args})")
        }
    }
}
