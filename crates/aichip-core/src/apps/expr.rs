//! The small expression language used by `compute`, `default` and `show_if`.
//!
//! Deliberately tiny. Field references, literals, arithmetic, comparison,
//! boolean operators, and a handful of functions — no property chains beyond a
//! bare name, no function definitions, no loops, no indexing, no assignment.
//! There is nothing here that runs long, allocates unboundedly, or reaches
//! outside the record it was given.
//!
//! ## Two implementations, one specification
//!
//! This is evaluated in Rust for anything authoritative — computed values,
//! defaults, the arguments an action step is given — and in TypeScript for
//! display-only conditions, because round-tripping to the server to decide
//! whether to show a button is absurd.
//!
//! Two implementations of one language drift. The defence is
//! [`expr_cases.json`], which both test suites read: it is the specification,
//! and the tests on either side are just two readers of it. A case added there
//! fails in whichever language has not caught up.
//!
//! ## Numbers are doubles
//!
//! Which is worth saying out loud in a module that will compute money. An
//! `amount * qty` is exact to about fifteen significant digits and then is not.
//! Storing a *stated* decimal keeps every digit (see `data::as_text`); a
//! *computed* one is as good as a double, and a manifest wanting more than that
//! should store the figure rather than derive it.

use std::collections::BTreeMap;
use std::fmt;

/// How deep an expression may nest.
///
/// The parser recurses, so this is what stands between a pathological manifest
/// and a blown stack. Reached in practice by nothing: five is a complicated
/// expression.
const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
}

impl Val {
    /// Whether this counts as true in a condition.
    ///
    /// Null, false, zero and the empty string are false; everything else is
    /// true. Chosen to match what someone writing `show_if: "category"` means,
    /// which is "when there is one".
    pub fn truthy(&self) -> bool {
        match self {
            Val::Null => false,
            Val::Bool(b) => *b,
            Val::Num(n) => *n != 0.0,
            Val::Str(s) => !s.is_empty(),
        }
    }

    pub fn as_json(&self) -> serde_json::Value {
        match self {
            Val::Null => serde_json::Value::Null,
            Val::Bool(b) => serde_json::Value::Bool(*b),
            Val::Num(n) => serde_json::Number::from_f64(*n)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            Val::Str(s) => serde_json::Value::String(s.clone()),
        }
    }

    pub fn from_json(v: &serde_json::Value) -> Val {
        match v {
            serde_json::Value::Null => Val::Null,
            serde_json::Value::Bool(b) => Val::Bool(*b),
            serde_json::Value::Number(n) => n.as_f64().map(Val::Num).unwrap_or(Val::Null),
            serde_json::Value::String(s) => Val::Str(s.clone()),
            other => Val::Str(other.to_string()),
        }
    }

    fn type_name(&self) -> &'static str {
        match self {
            Val::Null => "nothing",
            Val::Bool(_) => "a true/false",
            Val::Num(_) => "a number",
            Val::Str(_) => "text",
        }
    }
}

impl fmt::Display for Val {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Val::Null => Ok(()),
            Val::Bool(b) => write!(f, "{b}"),
            Val::Num(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    write!(f, "{}", *n as i64)
                } else {
                    write!(f, "{n}")
                }
            }
            Val::Str(s) => f.write_str(s),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprError(pub String);

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ExprError {}

type R<T> = Result<T, ExprError>;

fn err<T>(m: impl Into<String>) -> R<T> {
    Err(ExprError(m.into()))
}

/// The record an expression is evaluated against.
pub type Record = BTreeMap<String, Val>;

// ── Tokens ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Name(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
}

fn lex(src: &str) -> R<Vec<Tok>> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '\'' | '"' => {
                let quote = c;
                i += 1;
                let mut s = String::new();
                loop {
                    let Some(&ch) = chars.get(i) else {
                        return err("a quote was opened and never closed");
                    };
                    if ch == '\\' {
                        match chars.get(i + 1) {
                            Some(&next) => {
                                s.push(match next {
                                    'n' => '\n',
                                    't' => '\t',
                                    other => other,
                                });
                                i += 2;
                                continue;
                            }
                            None => return err("a quote was opened and never closed"),
                        }
                    }
                    if ch == quote {
                        i += 1;
                        break;
                    }
                    s.push(ch);
                    i += 1;
                }
                out.push(Tok::Str(s));
            }
            '0'..='9' => {
                let start = i;
                while chars.get(i).is_some_and(|c| c.is_ascii_digit()) {
                    i += 1;
                }
                if chars.get(i) == Some(&'.') && chars.get(i + 1).is_some_and(char::is_ascii_digit)
                {
                    i += 1;
                    while chars.get(i).is_some_and(|c| c.is_ascii_digit()) {
                        i += 1;
                    }
                }
                let text: String = chars[start..i].iter().collect();
                match text.parse() {
                    Ok(n) => out.push(Tok::Num(n)),
                    Err(_) => return err(format!("\"{text}\" is not a number")),
                }
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                // A dot is allowed inside a name so `record.field` is one
                // token. It is not property access — there are no objects — it
                // is a spelling of a field reference that reads naturally in a
                // prompt template.
                while chars
                    .get(i)
                    .is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
                {
                    i += 1;
                }
                out.push(Tok::Name(chars[start..i].iter().collect()));
            }
            _ => {
                const TWO: [&str; 6] = ["==", "!=", "<=", ">=", "&&", "||"];
                let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
                if let Some(op) = TWO.iter().find(|o| **o == two) {
                    out.push(Tok::Op(op));
                    i += 2;
                    continue;
                }
                const ONE: [&str; 8] = ["+", "-", "*", "/", "%", "<", ">", "!"];
                let one = c.to_string();
                match ONE.iter().find(|o| **o == one) {
                    Some(op) => {
                        out.push(Tok::Op(op));
                        i += 1;
                    }
                    // A bare `=` is the mistake worth naming: it is what
                    // everyone writes the first time they mean `==`.
                    None if c == '=' => return err("use == to compare, not ="),
                    None => return err(format!("\"{c}\" does not mean anything here")),
                }
            }
        }
    }
    Ok(out)
}

// ── Syntax ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Ast {
    Lit(Val),
    Field(String),
    Unary(&'static str, Box<Ast>),
    Binary(&'static str, Box<Ast>, Box<Ast>),
    Call(String, Vec<Ast>),
}

struct Parser {
    toks: Vec<Tok>,
    at: usize,
}

/// Binding power, loosest first. Ordinary precedence, so nothing surprises.
fn precedence(op: &str) -> Option<u8> {
    Some(match op {
        "||" => 1,
        "&&" => 2,
        "==" | "!=" => 3,
        "<" | "<=" | ">" | ">=" => 4,
        "+" | "-" => 5,
        "*" | "/" | "%" => 6,
        _ => return None,
    })
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }

    fn expr(&mut self, min: u8, depth: usize) -> R<Ast> {
        if depth > MAX_DEPTH {
            return err("this expression is nested too deeply");
        }
        let mut left = self.unary(depth)?;
        while let Some(Tok::Op(op)) = self.peek() {
            let op = *op;
            let Some(bp) = precedence(op) else { break };
            if bp < min {
                break;
            }
            self.at += 1;
            let right = self.expr(bp + 1, depth + 1)?;
            left = Ast::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self, depth: usize) -> R<Ast> {
        if depth > MAX_DEPTH {
            return err("this expression is nested too deeply");
        }
        if let Some(Tok::Op(op @ ("-" | "!"))) = self.peek() {
            let op = *op;
            self.at += 1;
            return Ok(Ast::Unary(op, Box::new(self.unary(depth + 1)?)));
        }
        self.atom(depth)
    }

    fn atom(&mut self, depth: usize) -> R<Ast> {
        let Some(tok) = self.peek().cloned() else {
            return err("the expression stops before it says anything");
        };
        self.at += 1;
        Ok(match tok {
            Tok::Num(n) => Ast::Lit(Val::Num(n)),
            Tok::Str(s) => Ast::Lit(Val::Str(s)),
            Tok::LParen => {
                let inner = self.expr(0, depth + 1)?;
                match self.peek() {
                    Some(Tok::RParen) => self.at += 1,
                    _ => return err("a bracket was opened and never closed"),
                }
                inner
            }
            Tok::Name(name) => match name.as_str() {
                "true" => Ast::Lit(Val::Bool(true)),
                "false" => Ast::Lit(Val::Bool(false)),
                "null" => Ast::Lit(Val::Null),
                _ if self.peek() == Some(&Tok::LParen) => {
                    self.at += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tok::RParen) {
                        loop {
                            args.push(self.expr(0, depth + 1)?);
                            match self.peek() {
                                Some(Tok::Comma) => self.at += 1,
                                _ => break,
                            }
                        }
                    }
                    match self.peek() {
                        Some(Tok::RParen) => self.at += 1,
                        _ => return err(format!("{name}( was opened and never closed")),
                    }
                    Ast::Call(name, args)
                }
                _ => Ast::Field(name),
            },
            Tok::RParen => return err("there is a ) with nothing to close"),
            Tok::Comma => return err("there is a , outside a function call"),
            Tok::Op(op) => return err(format!("\"{op}\" needs something before it")),
        })
    }
}

/// Read an expression. Does not evaluate it, so a manifest can be checked
/// without any data to check it against.
pub fn parse(src: &str) -> R<Ast> {
    let toks = lex(src)?;
    if toks.is_empty() {
        return err("this expression is empty");
    }
    let mut p = Parser { toks, at: 0 };
    let ast = p.expr(0, 0)?;
    if p.at != p.toks.len() {
        return err("there is more here than one expression");
    }
    Ok(ast)
}

/// Every field an expression reads. Used to check a manifest names real ones.
pub fn fields_used(ast: &Ast) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(ast: &Ast, out: &mut Vec<String>) {
        match ast {
            Ast::Field(name) => {
                let bare = name.strip_prefix("record.").unwrap_or(name).to_string();
                if !out.contains(&bare) {
                    out.push(bare);
                }
            }
            Ast::Unary(_, a) => walk(a, out),
            Ast::Binary(_, a, b) => {
                walk(a, out);
                walk(b, out);
            }
            Ast::Call(_, args) => args.iter().for_each(|a| walk(a, out)),
            Ast::Lit(_) => {}
        }
    }
    walk(ast, &mut out);
    out
}

// ── Evaluation ──────────────────────────────────────────────────────────────

/// Evaluate against a record.
///
/// `now` is passed in rather than read from the clock, so this function is
/// pure and the fixture corpus can pin what `today()` returns.
pub fn eval(ast: &Ast, record: &Record, now: &str) -> R<Val> {
    Ok(match ast {
        Ast::Lit(v) => v.clone(),
        Ast::Field(name) => {
            let bare = name.strip_prefix("record.").unwrap_or(name);
            // An absent field is null, not an error. A record is often
            // half-filled — a default is computed before the row exists — and
            // `show_if: "category == ''"` should say "no category yet" rather
            // than fail.
            record.get(bare).cloned().unwrap_or(Val::Null)
        }
        Ast::Unary(op, a) => {
            let a = eval(a, record, now)?;
            match *op {
                "!" => Val::Bool(!a.truthy()),
                "-" => match a {
                    Val::Num(n) => Val::Num(-n),
                    other => return err(format!("cannot negate {}", other.type_name())),
                },
                _ => return err(format!("\"{op}\" is not a prefix")),
            }
        }
        Ast::Binary(op, l, r) => {
            // Short-circuit before the other side is touched, so `paid && total
            // / qty > 1` does not divide by zero to decide something already
            // settled.
            match *op {
                "&&" => {
                    return Ok(Val::Bool(
                        eval(l, record, now)?.truthy() && eval(r, record, now)?.truthy(),
                    ))
                }
                "||" => {
                    return Ok(Val::Bool(
                        eval(l, record, now)?.truthy() || eval(r, record, now)?.truthy(),
                    ))
                }
                _ => {}
            }
            binary(op, eval(l, record, now)?, eval(r, record, now)?)?
        }
        Ast::Call(name, args) => {
            let vals: Vec<Val> = args
                .iter()
                .map(|a| eval(a, record, now))
                .collect::<R<Vec<_>>>()?;
            call(name, &vals, now)?
        }
    })
}

fn binary(op: &str, l: Val, r: Val) -> R<Val> {
    // Equality works on anything, including null, and does not coerce: "1" is
    // not 1. Anything else would make `status == 0` quietly true for an empty
    // status, which is the class of bug this language exists to not have.
    match op {
        "==" => return Ok(Val::Bool(l == r)),
        "!=" => return Ok(Val::Bool(l != r)),
        _ => {}
    }

    // Adding text to text joins it — the one place a type is not required to
    // match, because it is what everyone means by `first + ' ' + last`.
    if op == "+" {
        if let (Val::Str(a), b) = (&l, &r) {
            return Ok(Val::Str(format!("{a}{b}")));
        }
        if let (a, Val::Str(b)) = (&l, &r) {
            return Ok(Val::Str(format!("{a}{b}")));
        }
    }

    // Comparing text compares text.
    if let (Val::Str(a), Val::Str(b)) = (&l, &r) {
        return Ok(Val::Bool(match op {
            "<" => a < b,
            "<=" => a <= b,
            ">" => a > b,
            ">=" => a >= b,
            _ => return err(format!("\"{op}\" does not work on text")),
        }));
    }

    let (Val::Num(a), Val::Num(b)) = (&l, &r) else {
        // Null propagates instead of erroring: a row with no amount yet has no
        // total yet, which is a normal state rather than a broken manifest.
        if l == Val::Null || r == Val::Null {
            return Ok(Val::Null);
        }
        return err(format!(
            "cannot use \"{op}\" between {} and {}",
            l.type_name(),
            r.type_name()
        ));
    };

    Ok(match op {
        "+" => Val::Num(a + b),
        "-" => Val::Num(a - b),
        "*" => Val::Num(a * b),
        // Null rather than infinity or an error: a divisor of zero in a
        // computed column is missing data, and a row showing nothing is
        // better than a row showing inf.
        "/" => {
            if *b == 0.0 {
                Val::Null
            } else {
                Val::Num(a / b)
            }
        }
        "%" => {
            if *b == 0.0 {
                Val::Null
            } else {
                Val::Num(a % b)
            }
        }
        "<" => Val::Bool(a < b),
        "<=" => Val::Bool(a <= b),
        ">" => Val::Bool(a > b),
        ">=" => Val::Bool(a >= b),
        _ => return err(format!("\"{op}\" is not an operator")),
    })
}

/// Every function, and the whole list of them.
pub const FUNCTIONS: [&str; 9] = [
    "now", "today", "len", "lower", "upper", "round", "abs", "coalesce", "concat",
];

fn call(name: &str, args: &[Val], now: &str) -> R<Val> {
    let num = |i: usize| -> R<f64> {
        match args.get(i) {
            Some(Val::Num(n)) => Ok(*n),
            Some(other) => err(format!("{name} wants a number, not {}", other.type_name())),
            None => err(format!("{name} needs another argument")),
        }
    };
    Ok(match name {
        // The timestamp is supplied, not read, so this stays a pure function.
        "now" => Val::Str(now.to_string()),
        "today" => Val::Str(now.split('T').next().unwrap_or(now).to_string()),
        "len" => match args.first() {
            Some(Val::Str(s)) => Val::Num(s.chars().count() as f64),
            Some(Val::Null) | None => Val::Num(0.0),
            Some(other) => return err(format!("len wants text, not {}", other.type_name())),
        },
        "lower" => Val::Str(
            args.first()
                .unwrap_or(&Val::Null)
                .to_string()
                .to_lowercase(),
        ),
        "upper" => Val::Str(
            args.first()
                .unwrap_or(&Val::Null)
                .to_string()
                .to_uppercase(),
        ),
        "abs" => Val::Num(num(0)?.abs()),
        "round" => {
            let places = match args.get(1) {
                None => 0.0,
                Some(Val::Num(n)) => *n,
                Some(other) => {
                    return err(format!(
                        "round wants a number of places, not {}",
                        other.type_name()
                    ))
                }
            };
            let f = 10f64.powi(places.clamp(0.0, 10.0) as i32);
            Val::Num((num(0)? * f).round() / f)
        }
        "coalesce" => args
            .iter()
            .find(|v| **v != Val::Null)
            .cloned()
            .unwrap_or(Val::Null),
        "concat" => Val::Str(args.iter().map(|v| v.to_string()).collect()),
        _ => {
            return err(format!(
                "there is no function called {name} — there is {}",
                FUNCTIONS.join(", ")
            ))
        }
    })
}

/// Parse and evaluate in one go.
pub fn run(src: &str, record: &Record, now: &str) -> R<Val> {
    eval(&parse(src)?, record, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value as J;

    /// The shared specification. Both this and `web/src/lib/expr.test.ts` read
    /// it, which is the only thing keeping two implementations of one language
    /// honest with each other.
    const CASES: &str = include_str!("expr_cases.json");

    /// Compare a result to what the corpus expects.
    ///
    /// Numbers by value, not by representation: `serde_json` tells `1` and
    /// `1.0` apart and JavaScript does not, so a structural comparison would
    /// make the shared corpus impossible to satisfy on both sides at once.
    fn same(got: &J, want: &J) -> bool {
        match (got.as_f64(), want.as_f64()) {
            (Some(a), Some(b)) => (a - b).abs() < 1e-9,
            _ => got == want,
        }
    }

    #[test]
    fn every_shared_case_agrees() {
        let cases: Vec<J> = serde_json::from_str(CASES).expect("the corpus must be valid JSON");
        assert!(cases.len() > 30, "the corpus should be worth sharing");

        for case in &cases {
            let src = case["expr"].as_str().expect("every case needs an expr");
            let mut record = Record::new();
            if let Some(obj) = case.get("record").and_then(J::as_object) {
                for (k, v) in obj {
                    record.insert(k.clone(), Val::from_json(v));
                }
            }
            let now = case
                .get("now")
                .and_then(J::as_str)
                .unwrap_or("2026-08-02T04:00:00Z");

            let got = run(src, &record, now);
            if case.get("error").and_then(J::as_bool) == Some(true) {
                assert!(
                    got.is_err(),
                    "`{src}` should have been refused, got {got:?}"
                );
                continue;
            }
            let got = got.unwrap_or_else(|e| panic!("`{src}` failed: {e}"));
            assert!(
                same(&got.as_json(), &case["expect"]),
                "`{src}` gave {:?}, the corpus says {}",
                got.as_json(),
                case["expect"]
            );
        }
    }

    #[test]
    fn depth_is_bounded_rather_than_blowing_the_stack() {
        let deep = "(".repeat(5000) + "1" + &")".repeat(5000);
        assert!(parse(&deep).is_err());
        let chained = "1".to_string() + &"+1".repeat(10_000);
        // Left-associative chains do not recurse, so this is allowed to work —
        // what matters is that neither crashes.
        let _ = parse(&chained);
    }

    #[test]
    fn fields_used_finds_every_reference_once() {
        let ast = parse("amount * qty + record.amount + len(note)").unwrap();
        let mut used = fields_used(&ast);
        used.sort();
        assert_eq!(used, vec!["amount", "note", "qty"]);
    }

    #[test]
    fn a_bare_equals_says_what_was_meant() {
        assert!(parse("a = 1").unwrap_err().0.contains("use == to compare"));
    }

    #[test]
    fn an_unknown_function_lists_the_real_ones() {
        let e = run("frobnicate(1)", &Record::new(), "2026-01-01T00:00:00Z").unwrap_err();
        assert!(e.0.contains("coalesce"), "{e}");
    }
}
