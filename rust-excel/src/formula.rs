use crate::model::{Cell, Coord, Range, Value, Workbook};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const REF: &str = "#REF!";
const VALUE: &str = "#VALUE!";
const NAME: &str = "#NAME?";
const DIV: &str = "#DIV/0!";
const NUM: &str = "#NUM!";
const NA: &str = "#N/A";
const CIRC: &str = "#CIRC!";
const ERRORS: &[&str] = &[
    REF, VALUE, NAME, DIV, NUM, NA, CIRC, "#NULL!", "#SPILL!", "#CALC!",
];
type Calc<T> = Result<T, &'static str>;
type Key = (usize, Coord);

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    Quoted(String),
    Text(String),
    Number(f64),
    Error(&'static str),
    Op(&'static str),
    End,
}
#[derive(Clone, Debug)]
struct Lex {
    token: Token,
    start: usize,
    end: usize,
}
fn lex(s: &str) -> Calc<Vec<Lex>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let start = i;
        let c = s[i..].chars().next().ok_or(VALUE)?;
        i += c.len_utf8();
        let token = match c {
            c if c.is_whitespace() => continue,
            '\'' | '"' => {
                let mut text = String::new();
                loop {
                    let next = s[i..].chars().next().ok_or(VALUE)?;
                    i += next.len_utf8();
                    if next == c {
                        if s[i..].starts_with(c) {
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    text.push(next);
                }
                if c == '"' {
                    Token::Text(text)
                } else {
                    Token::Quoted(text)
                }
            }
            c if c.is_ascii_digit()
                || (c == '.' && s.as_bytes().get(i).is_some_and(u8::is_ascii_digit)) =>
            {
                while s
                    .as_bytes()
                    .get(i)
                    .is_some_and(|b| b.is_ascii_digit() || *b == b'.')
                {
                    i += 1;
                }
                if s.as_bytes()
                    .get(i)
                    .is_some_and(|b| matches!(b, b'E' | b'e'))
                {
                    let before = i;
                    i += 1;
                    if s.as_bytes()
                        .get(i)
                        .is_some_and(|b| matches!(b, b'+' | b'-'))
                    {
                        i += 1;
                    }
                    let digits = i;
                    while s.as_bytes().get(i).is_some_and(u8::is_ascii_digit) {
                        i += 1;
                    }
                    if digits == i {
                        i = before;
                    }
                }
                Token::Number(s[start..i].parse().map_err(|_| VALUE)?)
            }
            c if c.is_alphabetic() || matches!(c, '_' | '$') => {
                while let Some(next) = s[i..]
                    .chars()
                    .next()
                    .filter(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '.'))
                {
                    i += next.len_utf8();
                }
                Token::Word(s[start..i].into())
            }
            '#' => {
                let e = ERRORS
                    .iter()
                    .copied()
                    .find(|e| {
                        s[start..]
                            .get(..e.len())
                            .is_some_and(|p| p.eq_ignore_ascii_case(e))
                    })
                    .ok_or(VALUE)?;
                i = start + e.len();
                Token::Error(e)
            }
            '+' => Token::Op("+"),
            '-' => Token::Op("-"),
            '*' => Token::Op("*"),
            '/' => Token::Op("/"),
            '^' => Token::Op("^"),
            '&' => Token::Op("&"),
            '(' => Token::Op("("),
            ')' => Token::Op(")"),
            ',' | ';' => Token::Op(","),
            '!' => Token::Op("!"),
            ':' => Token::Op(":"),
            '%' => Token::Op("%"),
            '=' => Token::Op("="),
            '<' if s[i..].starts_with('=') => {
                i += 1;
                Token::Op("<=")
            }
            '>' if s[i..].starts_with('=') => {
                i += 1;
                Token::Op(">=")
            }
            '<' if s[i..].starts_with('>') => {
                i += 1;
                Token::Op("<>")
            }
            '<' => Token::Op("<"),
            '>' => Token::Op(">"),
            _ => return Err(VALUE),
        };
        out.push(Lex {
            token,
            start,
            end: i,
        });
        if out.len() > 512 {
            return Err(VALUE);
        }
    }
    out.push(Lex {
        token: Token::End,
        start: i,
        end: i,
    });
    Ok(out)
}

#[derive(Clone)]
enum Expr {
    Literal(Value),
    Error(&'static str),
    Ref(Option<String>, Range),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}
struct Parser {
    tokens: Vec<Lex>,
    at: usize,
}
impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.at].token
    }
    fn take(&mut self) -> Token {
        let t = self.peek().clone();
        if t != Token::End {
            self.at += 1;
        }
        t
    }
    fn eat(&mut self, op: &'static str) -> bool {
        if *self.peek() == Token::Op(op) {
            self.take();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, op: &'static str) -> Calc<()> {
        if self.eat(op) { Ok(()) } else { Err(VALUE) }
    }
    fn address(&mut self) -> Calc<String> {
        match self.take() {
            Token::Word(s) => Ok(s),
            Token::Error(e) => Err(e),
            _ => Err(REF),
        }
    }
    fn reference(&mut self, sheet: Option<String>, first: String) -> Calc<Expr> {
        let second = if self.eat(":") {
            let word = self.address()?;
            if self.eat("!") {
                if !sheet
                    .as_ref()
                    .is_some_and(|s| s.eq_ignore_ascii_case(&word))
                {
                    return Err(REF);
                }
                self.address()?
            } else {
                word
            }
        } else {
            first.clone()
        };
        let range = Range::parse(&format!("{first}:{second}")).map_err(|_| REF)?;
        Ok(Expr::Ref(sheet, range))
    }
    fn expr(&mut self, min_bp: u8) -> Calc<Expr> {
        let mut left = match self.take() {
            Token::Number(n) if n.is_finite() => Expr::Literal(Value::Number(n)),
            Token::Number(_) => Expr::Error(NUM),
            Token::Text(s) => Expr::Literal(Value::Text(s)),
            Token::Error(e) => Expr::Error(e),
            Token::Op(op @ ("+" | "-")) => Expr::Unary(op, Box::new(self.expr(7)?)),
            Token::Op("(") => {
                let x = self.expr(0)?;
                self.expect(")")?;
                x
            }
            Token::Word(s) | Token::Quoted(s) if self.eat("!") => {
                let a = self.address()?;
                self.reference(Some(s), a)?
            }
            Token::Word(s) if self.eat("(") => {
                let mut args = Vec::new();
                if !self.eat(")") {
                    loop {
                        args.push(if matches!(self.peek(), Token::Op("," | ")")) {
                            Expr::Literal(Value::Blank)
                        } else {
                            self.expr(0)?
                        });
                        if !self.eat(",") {
                            break;
                        }
                    }
                    self.expect(")")?;
                }
                let name = s.to_ascii_uppercase();
                Expr::Call(name.strip_prefix("_XLFN.").unwrap_or(&name).into(), args)
            }
            Token::Word(s) if s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("false") => {
                Expr::Literal(Value::Bool(s.eq_ignore_ascii_case("true")))
            }
            Token::Word(s) if Coord::parse(&s).is_ok() || *self.peek() == Token::Op(":") => {
                self.reference(None, s)?
            }
            Token::Word(_) => Expr::Error(NAME),
            _ => return Err(VALUE),
        };
        loop {
            let Token::Op(op) = *self.peek() else {
                break;
            };
            let bp = match op {
                "=" | "<>" | "<" | "<=" | ">" | ">=" => 1,
                "&" => 2,
                "+" | "-" => 3,
                "*" | "/" => 4,
                "^" => 5,
                "%" => 6,
                _ => break,
            };
            if bp < min_bp {
                break;
            }
            self.take();
            left = if op == "%" {
                Expr::Unary(op, Box::new(left))
            } else {
                Expr::Binary(op, Box::new(left), Box::new(self.expr(bp + 1)?))
            };
        }
        Ok(left)
    }
}
fn parse(s: &str) -> Expr {
    let result = (|| {
        let mut p = Parser {
            tokens: lex(s)?,
            at: 0,
        };
        let result = p.expr(0)?;
        if *p.peek() != Token::End {
            return Err(VALUE);
        }
        Ok(result)
    })();
    result.unwrap_or_else(Expr::Error)
}

#[derive(Clone, Copy)]
pub(crate) enum RefEdit<'a> {
    Rename(&'a str),
    DeleteSheet,
    Shift {
        rows: bool,
        at: u32,
        count: u32,
        insert: bool,
    },
}
fn ref_text(original: &str, c: Coord) -> String {
    let dollar_col = original.starts_with('$');
    let dollar_row = original.trim_start_matches('$').contains('$');
    format!(
        "{}{}{}{}",
        if dollar_col { "$" } else { "" },
        Coord::column(c.col),
        if dollar_row { "$" } else { "" },
        c.row
    )
}
pub(crate) fn rewrite(formula: &str, current: &str, target: &str, edit: RefEdit<'_>) -> String {
    let Ok(tokens) = lex(formula) else {
        return formula.into();
    };
    let mut patches = Vec::new();
    let mut i = 0;
    while i + 1 < tokens.len() {
        let start = i;
        let explicit = matches!(tokens[i].token, Token::Word(_) | Token::Quoted(_))
            && tokens[i + 1].token == Token::Op("!");
        let sheet = if explicit {
            match &tokens[i].token {
                Token::Word(s) | Token::Quoted(s) => s.as_str(),
                _ => unreachable!(),
            }
        } else {
            current
        };
        if explicit {
            i += 2;
        }
        let Some(a) = tokens.get(i) else { break };
        let Token::Word(raw) = &a.token else {
            i += 1;
            continue;
        };
        let Ok(mut first) = Coord::parse(raw) else {
            i += 1;
            continue;
        };
        if tokens.get(i + 1).is_some_and(|t| t.token == Token::Op("(")) {
            i += 1;
            continue;
        }
        let mut last = None;
        if tokens.get(i + 1).is_some_and(|t| t.token == Token::Op(":"))
            && let Some(t) = tokens.get(i + 2)
            && let Token::Word(s) = &t.token
            && let Ok(c) = Coord::parse(s)
        {
            last = Some((t, s, c));
        }
        i += if last.is_some() { 3 } else { 1 };
        if sheet.to_lowercase() != target.to_lowercase() {
            continue;
        }
        let lo = tokens[start].start;
        let hi = tokens[i - 1].end;
        match edit {
            RefEdit::Rename(name) if explicit => patches.push((
                lo,
                tokens[start].end,
                format!("'{}'", name.replace('\'', "''")),
            )),
            RefEdit::DeleteSheet if explicit => patches.push((lo, hi, REF.into())),
            RefEdit::Shift {
                rows,
                at,
                count,
                insert,
            } => {
                let get = |c: Coord| if rows { c.row } else { c.col };
                let set = |c: &mut Coord, n| if rows { c.row = n } else { c.col = n };
                let a1 = get(first);
                let a2 = last.as_ref().map_or(a1, |(_, _, c)| get(*c));
                let (low, high) = (a1.min(a2), a1.max(a2));
                let end = at + count - 1;
                let moved = if insert {
                    Some((
                        low + if low >= at { count } else { 0 },
                        high + if high >= at { count } else { 0 },
                    ))
                } else if low >= at && high <= end {
                    None
                } else {
                    Some((
                        if low > end { low - count } else { low.min(at) },
                        if high > end {
                            high - count
                        } else {
                            high.min(at - 1)
                        },
                    ))
                };
                if let Some((n1, n2)) = moved.filter(|(_, n)| {
                    *n <= if rows {
                        crate::model::ROWS
                    } else {
                        crate::model::COLS
                    }
                }) {
                    set(&mut first, if a1 <= a2 { n1 } else { n2 });
                    patches.push((a.start, a.end, ref_text(raw, first)));
                    if let Some((t, s, mut c)) = last {
                        set(&mut c, if a1 <= a2 { n2 } else { n1 });
                        patches.push((t.start, t.end, ref_text(s, c)));
                    }
                } else {
                    patches.push((lo, hi, REF.into()));
                }
            }
            _ => {}
        }
    }
    let mut out = formula.to_owned();
    for (a, b, v) in patches.into_iter().rev() {
        out.replace_range(a..b, &v);
    }
    out
}

type Grid = BTreeMap<u32, BTreeMap<u32, Calc<Value>>>;
struct Context {
    names: BTreeMap<String, usize>,
    grids: Vec<Grid>,
}
impl Context {
    fn sheet(&self, active: usize, name: &Option<String>) -> Calc<usize> {
        match name {
            Some(n) => self.names.get(&n.to_lowercase()).copied().ok_or(REF),
            None => Ok(active),
        }
    }
    fn get(&self, s: usize, c: Coord) -> Calc<Value> {
        self.grids[s]
            .get(&c.row)
            .and_then(|row| row.get(&c.col))
            .cloned()
            .unwrap_or(Ok(Value::Blank))
    }
    fn set(&mut self, (s, c): Key, value: Calc<Value>) {
        self.grids[s].entry(c.row).or_default().insert(c.col, value);
    }
    fn sparse(&self, s: usize, r: Range) -> impl Iterator<Item = (Coord, Calc<Value>)> + '_ {
        self.grids[s]
            .range(r.start.row..=r.end.row)
            .flat_map(move |(&row, entries)| {
                entries
                    .range(r.start.col..=r.end.col)
                    .map(move |(&col, v)| (Coord { row, col }, v.clone()))
            })
    }
}
#[derive(Clone)]
enum Arg {
    Scalar(Value),
    Area(usize, Range),
}
impl Arg {
    fn scalar(&self, cx: &Context) -> Calc<Value> {
        match self {
            Self::Scalar(v) => Ok(v.clone()),
            Self::Area(s, r) => cx.get(*s, r.start),
        }
    }
    fn dimensions(&self) -> (u32, u32) {
        match self {
            Self::Scalar(_) => (1, 1),
            Self::Area(_, r) => (r.end.row - r.start.row + 1, r.end.col - r.start.col + 1),
        }
    }
    fn get(&self, cx: &Context, row: u32, col: u32) -> Calc<Value> {
        let (h, w) = self.dimensions();
        if row >= h || col >= w {
            return Err(REF);
        }
        match self {
            Self::Scalar(v) => Ok(v.clone()),
            Self::Area(s, r) => cx.get(
                *s,
                Coord {
                    row: r.start.row + row,
                    col: r.start.col + col,
                },
            ),
        }
    }
    fn values<'a>(&'a self, cx: &'a Context) -> Box<dyn Iterator<Item = Calc<Value>> + 'a> {
        match self {
            Self::Scalar(v) => Box::new(std::iter::once(Ok(v.clone()))),
            Self::Area(s, r) => Box::new(cx.sparse(*s, *r).map(|(_, v)| v)),
        }
    }
}
fn num(v: &Value) -> Calc<f64> {
    match v {
        Value::Number(n) => Ok(*n),
        Value::Bool(b) => Ok(u8::from(*b).into()),
        Value::Blank => Ok(0.),
        Value::Text(s) => s
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or(VALUE),
    }
}
fn truth(v: &Value) -> Calc<bool> {
    match v {
        Value::Text(s) if s.eq_ignore_ascii_case("TRUE") => Ok(true),
        Value::Text(s) if s.eq_ignore_ascii_case("FALSE") => Ok(false),
        _ => Ok(num(v)? != 0.),
    }
}
fn finite(n: f64) -> Calc<Value> {
    if n.is_finite() {
        Ok(Value::Number(n))
    } else {
        Err(NUM)
    }
}
fn cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Value::Text(a), Value::Text(b)) => a.to_lowercase().cmp(&b.to_lowercase()),
        (Value::Blank, Value::Text(b)) => "".cmp(b.as_str()),
        (Value::Text(a), Value::Blank) => a.as_str().cmp(""),
        (Value::Text(_), _) => Ordering::Greater,
        (_, Value::Text(_)) => Ordering::Less,
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        (Value::Bool(_), Value::Number(_)) => Ordering::Greater,
        (Value::Number(_), Value::Bool(_)) => Ordering::Less,
        _ => num(a)
            .unwrap_or(0.)
            .partial_cmp(&num(b).unwrap_or(0.))
            .unwrap_or(Ordering::Equal),
    }
}
fn compare(op: &str, a: &Value, b: &Value) -> bool {
    let order = cmp(a, b);
    match op {
        "=" => order.is_eq(),
        "<>" => !order.is_eq(),
        "<" => order.is_lt(),
        ">" => order.is_gt(),
        "<=" => !order.is_gt(),
        ">=" => !order.is_lt(),
        _ => false,
    }
}
fn matches_pattern(pattern: &str, text: &str) -> bool {
    #[derive(Clone, Copy)]
    enum Part {
        Star,
        One,
        Literal(char),
    }
    let pattern = pattern.to_lowercase();
    let text: Vec<_> = text.to_lowercase().chars().collect();
    let mut chars = pattern.chars().peekable();
    let mut parts = Vec::new();
    while let Some(c) = chars.next() {
        parts.push(match c {
            '*' => Part::Star,
            '?' => Part::One,
            '~' if chars.peek().is_some_and(|c| matches!(c, '*' | '?' | '~')) => {
                Part::Literal(chars.next().unwrap_or('~'))
            }
            _ => Part::Literal(c),
        });
    }
    let (mut p, mut t, mut retry) = (0, 0, None);
    while t < text.len() {
        match parts.get(p) {
            Some(Part::One) => {
                p += 1;
                t += 1;
            }
            Some(Part::Literal(c)) if *c == text[t] => {
                p += 1;
                t += 1;
            }
            Some(Part::Star) => {
                p += 1;
                retry = Some((p, t));
            }
            _ => {
                if let Some((next, used)) = retry {
                    p = next;
                    t = used + 1;
                    retry = Some((p, t));
                } else {
                    return false;
                }
            }
        }
    }
    parts[p..].iter().all(|p| matches!(p, Part::Star))
}
fn criterion(v: &Value, needle: &Value) -> bool {
    if let Value::Text(s) = needle {
        let op = [">=", "<=", "<>", "=", ">", "<"]
            .into_iter()
            .find(|op| s.starts_with(op));
        let tail = &s[op.map_or(0, str::len)..];
        if let Ok(n) = tail.parse::<f64>() {
            if let Ok(actual) = num(v) {
                return compare(op.unwrap_or("="), &Value::Number(actual), &Value::Number(n));
            }
        }
        if let Some(op @ ("<>" | "=")) = op.or(Some("=")) {
            if tail.contains(['*', '?', '~']) {
                return matches_pattern(tail, &v.text()) == (op == "=");
            }
        }
        return compare(op.unwrap_or("="), v, &Value::Text(tail.into()));
    }
    compare("=", v, needle)
}
fn args_len(args: &[Expr], min: usize, max: usize) -> Calc<()> {
    if (min..=max).contains(&args.len()) {
        Ok(())
    } else {
        Err(VALUE)
    }
}
fn idx(n: f64) -> Calc<u32> {
    if n >= 1. && n <= f64::from(u32::MAX) {
        Ok(n.trunc() as u32 - 1)
    } else {
        Err(VALUE)
    }
}

fn evaluate(x: &Expr, cx: &Context, s: usize) -> Calc<Arg> {
    let cell = |x: &Expr| evaluate(x, cx, s)?.scalar(cx);
    let result = match x {
        Expr::Literal(v) => v.clone(),
        Expr::Error(e) => return Err(e),
        Expr::Ref(sheet, r) => return Ok(Arg::Area(cx.sheet(s, sheet)?, *r)),
        Expr::Unary(op, x) => {
            let x = num(&cell(x)?)?;
            finite(match *op {
                "-" => -x,
                "%" => x / 100.,
                _ => x,
            })?
        }
        Expr::Binary(op, a, b) => {
            let (a, b) = (cell(a)?, cell(b)?);
            match *op {
                "&" => Value::Text(a.text() + &b.text()),
                "=" | "<>" | "<" | "<=" | ">" | ">=" => Value::Bool(compare(op, &a, &b)),
                _ => {
                    let (a, b) = (num(&a)?, num(&b)?);
                    finite(match *op {
                        "+" => a + b,
                        "-" => a - b,
                        "*" => a * b,
                        "/" if b == 0. => return Err(DIV),
                        "/" => a / b,
                        "^" => a.powf(b),
                        _ => return Err(VALUE),
                    })?
                }
            }
        }
        Expr::Call(name, args) => call(name, args, cx, s)?,
    };
    Ok(Arg::Scalar(result))
}

fn call(name: &str, args: &[Expr], cx: &Context, s: usize) -> Calc<Value> {
    let eval = |i: usize| evaluate(args.get(i).ok_or(VALUE)?, cx, s);
    let val = |i| eval(i)?.scalar(cx);
    let n = |i| num(&val(i)?);
    let t = |i| Ok::<_, &str>(val(i)?.text());
    let len = |min, max| args_len(args, min, max);
    match name {
        "IF" => {
            len(2, 3)?;
            if truth(&val(0)?)? {
                val(1)
            } else if args.len() == 3 {
                val(2)
            } else {
                Ok(Value::Bool(false))
            }
        }
        "IFS" => {
            if args.is_empty() || !args.len().is_multiple_of(2) {
                return Err(VALUE);
            }
            for i in (0..args.len()).step_by(2) {
                if truth(&val(i)?)? {
                    return val(i + 1);
                }
            }
            Err(NA)
        }
        "IFERROR" => {
            len(2, 2)?;
            val(0).or_else(|_| val(1))
        }
        "NOT" => {
            len(1, 1)?;
            Ok(Value::Bool(!truth(&val(0)?)?))
        }
        "AND" | "OR" => {
            let mut any = false;
            let mut result = name == "AND";
            for i in 0..args.len() {
                let a = eval(i)?;
                for v in a.values(cx) {
                    let v = v?;
                    if matches!(a, Arg::Area(..)) && matches!(v, Value::Blank | Value::Text(_)) {
                        continue;
                    }
                    let b = truth(&v)?;
                    any = true;
                    if name == "AND" {
                        result &= b;
                    } else {
                        result |= b;
                    }
                }
            }
            if any {
                Ok(Value::Bool(result))
            } else {
                Err(VALUE)
            }
        }
        "SUM" | "AVERAGE" | "COUNT" | "COUNTA" | "MIN" | "MAX" | "MEDIAN" => {
            let mut numbers = Vec::new();
            let mut count = 0_u64;
            for i in 0..args.len() {
                let a = match eval(i) {
                    Ok(a) => a,
                    Err(_) if name == "COUNT" => continue,
                    Err(_) if name == "COUNTA" => {
                        count += 1;
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                for v in a.values(cx) {
                    if name == "COUNTA" {
                        if !matches!(v, Ok(Value::Blank)) {
                            count += 1;
                        }
                        continue;
                    }
                    let v = match v {
                        Ok(v) => v,
                        Err(_) if name == "COUNT" => continue,
                        Err(e) => return Err(e),
                    };
                    match v {
                        Value::Number(n) => numbers.push(n),
                        Value::Blank => {}
                        _ if matches!(a, Arg::Scalar(_)) => {
                            if let Ok(n) = num(&v) {
                                numbers.push(n);
                            }
                        }
                        _ => {}
                    }
                }
            }
            let size = numbers.len();
            finite(match name {
                "SUM" => numbers.iter().sum(),
                "COUNT" => size as f64,
                "COUNTA" => count as f64,
                "AVERAGE" if size == 0 => return Err(DIV),
                "AVERAGE" => numbers.iter().sum::<f64>() / size as f64,
                "MIN" => numbers.into_iter().reduce(f64::min).unwrap_or(0.),
                "MAX" => numbers.into_iter().reduce(f64::max).unwrap_or(0.),
                _ if size == 0 => return Err(NUM),
                _ => {
                    numbers.sort_by(f64::total_cmp);
                    if size % 2 == 0 {
                        numbers[size / 2 - 1] / 2. + numbers[size / 2] / 2.
                    } else {
                        numbers[size / 2]
                    }
                }
            })
        }
        "COUNTIF" | "SUMIF" => {
            len(2, if name == "COUNTIF" { 2 } else { 3 })?;
            let a = eval(0)?;
            let needle = val(1)?;
            let sum = if args.len() == 3 { eval(2)? } else { a.clone() };
            let mut result = 0.;
            if let Arg::Area(sheet, range) = a {
                if name == "COUNTIF" {
                    let mut represented = 0_u64;
                    for (_, v) in cx.sparse(sheet, range) {
                        represented += 1;
                        if v.is_ok_and(|v| criterion(&v, &needle)) {
                            result += 1.;
                        }
                    }
                    if criterion(&Value::Blank, &needle) {
                        result += (range.size() - represented) as f64;
                    }
                } else if let Arg::Area(sum_sheet, sum_range) = sum {
                    let covered = sumif_extent(range, sum_range.start);
                    for (coord, v) in cx.sparse(sum_sheet, covered) {
                        if a.get(
                            cx,
                            coord.row - covered.start.row,
                            coord.col - covered.start.col,
                        )
                        .is_ok_and(|v| criterion(&v, &needle))
                            && let Value::Number(n) = v?
                        {
                            result += n;
                        }
                    }
                } else if a.get(cx, 0, 0).is_ok_and(|v| criterion(&v, &needle))
                    && let Value::Number(n) = sum.scalar(cx)?
                {
                    result += n;
                }
            } else if criterion(&a.scalar(cx)?, &needle) {
                result = if name == "COUNTIF" {
                    1.
                } else {
                    match sum.scalar(cx)? {
                        Value::Number(n) => n,
                        _ => 0.,
                    }
                };
            }
            finite(result)
        }
        "ROUND" | "ROUNDUP" | "ROUNDDOWN" => {
            len(2, 2)?;
            let (a, d) = (n(0)?, n(1)?.trunc().clamp(-308., 308.) as i32);
            let f = 10f64.powi(d);
            let x = a * f;
            finite(
                match name {
                    "ROUND" => x.round(),
                    "ROUNDUP" => x.abs().ceil().copysign(x),
                    _ => x.trunc(),
                } / f,
            )
        }
        "ABS" | "INT" | "SQRT" => {
            len(1, 1)?;
            let a = n(0)?;
            finite(match name {
                "ABS" => a.abs(),
                "INT" => a.floor(),
                _ => a.sqrt(),
            })
        }
        "MOD" | "POWER" => {
            len(2, 2)?;
            let (a, b) = (n(0)?, n(1)?);
            if name == "MOD" {
                if b == 0. {
                    return Err(DIV);
                }
                finite(a - b * (a / b).floor())
            } else {
                finite(a.powf(b))
            }
        }
        "CONCAT" | "CONCATENATE" => {
            let mut out = String::new();
            for i in 0..args.len() {
                let a = eval(i)?;
                for v in a.values(cx) {
                    out.push_str(&v?.text());
                    if out.encode_utf16().count() > 32767 {
                        return Err(VALUE);
                    }
                }
            }
            Ok(Value::Text(out))
        }
        "LEN" | "TRIM" | "UPPER" | "LOWER" => {
            len(1, 1)?;
            let s = t(0)?;
            Ok(match name {
                "LEN" => Value::Number(s.chars().count() as f64),
                "TRIM" => Value::Text(
                    s.split(' ')
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                "UPPER" => Value::Text(s.to_uppercase()),
                _ => Value::Text(s.to_lowercase()),
            })
        }
        "LEFT" | "RIGHT" | "MID" => {
            len(
                if name == "MID" { 3 } else { 1 },
                if name == "MID" { 3 } else { 2 },
            )?;
            let text = t(0)?;
            let count = if name == "MID" {
                n(2)?
            } else if args.len() == 2 {
                n(1)?
            } else {
                1.
            };
            if count < 0. {
                return Err(VALUE);
            }
            let k = count as usize;
            Ok(Value::Text(match name {
                "LEFT" => text.chars().take(k).collect(),
                "RIGHT" => text
                    .chars()
                    .skip(text.chars().count().saturating_sub(k))
                    .collect(),
                _ => text.chars().skip(idx(n(1)?)? as usize).take(k).collect(),
            }))
        }
        "INDEX" => {
            len(2, 3)?;
            let a = eval(0)?;
            let i = idx(n(1)?)?;
            if args.len() == 2 && a.dimensions().0 == 1 {
                a.get(cx, 0, i)
            } else {
                a.get(cx, i, if args.len() == 3 { idx(n(2)?)? } else { 0 })
            }
        }
        "MATCH" | "VLOOKUP" => {
            len(
                if name == "MATCH" { 2 } else { 3 },
                if name == "MATCH" { 3 } else { 4 },
            )?;
            let needle = val(0)?;
            let a = eval(1)?;
            let (h, w) = a.dimensions();
            let vector = name == "MATCH";
            if vector && h > 1 && w > 1 {
                return Err(NA);
            }
            let mode = if vector {
                if args.len() == 3 {
                    n(2)?.signum() as i8 * i8::from(n(2)? != 0.)
                } else {
                    1
                }
            } else if args.len() == 4 && !truth(&val(3)?)? {
                0
            } else {
                1
            };
            let column = if vector { 0 } else { idx(n(2)?)? };
            if column >= w {
                return Err(REF);
            }
            let horizontal = vector && h == 1;
            let mut best: Option<(u32, Value)> = None;
            for i in 0..if horizontal { w } else { h } {
                let Ok(v) = (if horizontal {
                    a.get(cx, 0, i)
                } else {
                    a.get(cx, i, 0)
                }) else {
                    continue;
                };
                if mode == 0 {
                    if cmp(&v, &needle).is_eq()
                        || matches!((&needle,&v),(Value::Text(p),Value::Text(t)) if p.contains(['*','?','~']) && matches_pattern(p,t))
                    {
                        best = Some((i, v));
                        break;
                    }
                } else if (if mode > 0 {
                    !cmp(&v, &needle).is_gt()
                } else {
                    !cmp(&v, &needle).is_lt()
                }) && best.as_ref().is_none_or(|(_, old)| {
                    if mode > 0 {
                        !cmp(&v, old).is_lt()
                    } else {
                        !cmp(&v, old).is_gt()
                    }
                }) {
                    best = Some((i, v));
                }
            }
            let (i, _) = best.ok_or(NA)?;
            if vector {
                Ok(Value::Number(f64::from(i + 1)))
            } else {
                a.get(cx, i, column)
            }
        }
        _ => Err(NAME),
    }
}

fn sumif_extent(criteria: Range, start: Coord) -> Range {
    Range {
        start,
        end: Coord {
            row: (start.row + criteria.end.row - criteria.start.row).min(crate::model::ROWS),
            col: (start.col + criteria.end.col - criteria.start.col).min(crate::model::COLS),
        },
    }
}

fn collect_refs(x: &Expr, out: &mut Vec<(Option<String>, Range)>) {
    match x {
        Expr::Ref(s, r) => out.push((s.clone(), *r)),
        Expr::Unary(_, a) => collect_refs(a, out),
        Expr::Binary(_, a, b) => {
            collect_refs(a, out);
            collect_refs(b, out);
        }
        Expr::Call(name, args) => {
            for a in args {
                collect_refs(a, out);
            }
            // SUMIF uses the criteria area's shape from the top-left of sum_range.
            if name == "SUMIF"
                && let [Expr::Ref(_, criteria), _, Expr::Ref(sheet, sum)] = args.as_slice()
            {
                out.push((sheet.clone(), sumif_extent(*criteria, sum.start)));
            }
        }
        _ => {}
    }
}

// Iterative Kosaraju: only the firmly cyclic components are marked, not their downstream callers.
fn cycles(graph: &[Vec<usize>], reverse: &[Vec<usize>]) -> Vec<bool> {
    let mut seen = vec![false; graph.len()];
    let mut order = Vec::new();
    for i in 0..graph.len() {
        if seen[i] {
            continue;
        }
        seen[i] = true;
        let mut stack = vec![(i, 0)];
        while let Some((node, edge)) = stack.last_mut() {
            if let Some(&next) = graph[*node].get(*edge) {
                *edge += 1;
                if !seen[next] {
                    seen[next] = true;
                    stack.push((next, 0));
                }
            } else {
                order.push(*node);
                stack.pop();
            }
        }
    }
    seen.fill(false);
    let mut cycle = vec![false; graph.len()];
    for i in order.into_iter().rev() {
        if seen[i] {
            continue;
        }
        seen[i] = true;
        let mut stack = vec![i];
        let mut component = Vec::new();
        while let Some(n) = stack.pop() {
            component.push(n);
            for &p in &reverse[n] {
                if !seen[p] {
                    seen[p] = true;
                    stack.push(p);
                }
            }
        }
        if component.len() > 1 || graph[i].contains(&i) {
            for n in component {
                cycle[n] = true;
            }
        }
    }
    cycle
}

pub fn recalculate(wb: &mut Workbook) {
    let mut cx = Context {
        names: wb
            .sheets
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name.to_lowercase(), i))
            .collect(),
        grids: vec![Grid::new(); wb.sheets.len()],
    };
    let mut index = vec![BTreeMap::<u32, BTreeMap<u32, usize>>::new(); wb.sheets.len()];
    let mut nodes = Vec::new();
    for (s, sheet) in wb.sheets.iter().enumerate() {
        for (a, cell) in &sheet.cells {
            let Ok(coord) = Coord::parse(a) else { continue };
            if let Some(f) = &cell.formula {
                index[s]
                    .entry(coord.row)
                    .or_default()
                    .insert(coord.col, nodes.len());
                nodes.push(((s, coord), a.clone(), parse(f.trim_start_matches('='))));
            } else {
                cx.set(
                    (s, coord),
                    if cell.error {
                        Err(ERRORS
                            .iter()
                            .copied()
                            .find(|e| *e == cell.display)
                            .unwrap_or(VALUE))
                    } else {
                        Ok(cell.value.clone())
                    },
                );
            }
        }
    }
    let mut graph = vec![Vec::new(); nodes.len()];
    let mut reverse = graph.clone();
    for (i, ((s, _), _, exp)) in nodes.iter().enumerate() {
        let mut refs = Vec::new();
        collect_refs(exp, &mut refs);
        let mut deps = BTreeSet::new();
        for (name, r) in refs {
            if let Ok(s) = cx.sheet(*s, &name) {
                for (_, row) in index[s].range(r.start.row..=r.end.row) {
                    deps.extend(row.range(r.start.col..=r.end.col).map(|(_, i)| *i));
                }
            }
        }
        graph[i].extend(deps);
        for &d in &graph[i] {
            reverse[d].push(i);
        }
    }
    let cycle = cycles(&graph, &reverse);
    let mut queue = VecDeque::new();
    let mut degree = vec![0; nodes.len()];
    for (i, (key, _, _)) in nodes.iter().enumerate() {
        if cycle[i] {
            cx.set(*key, Err(CIRC));
        } else {
            degree[i] = graph[i].iter().filter(|&&d| !cycle[d]).count();
            if degree[i] == 0 {
                queue.push_back(i);
            }
        }
    }
    while let Some(i) = queue.pop_front() {
        let (key, _, exp) = &nodes[i];
        let value = evaluate(exp, &cx, key.0).and_then(|v| v.scalar(&cx));
        cx.set(*key, value);
        for &n in &reverse[i] {
            if !cycle[n] {
                degree[n] -= 1;
                if degree[n] == 0 {
                    queue.push_back(n);
                }
            }
        }
    }
    for ((s, c), a, _) in nodes {
        if let Some(cell) = wb.sheets[s].cells.get_mut(&a) {
            let value = cx.get(s, c);
            cell.error = value.is_err();
            cell.value = value.unwrap_or_else(|e| Value::Text(e.into()));
        }
    }
    for sheet in &mut wb.sheets {
        for cell in sheet.cells.values_mut() {
            refresh_display(cell);
        }
    }
}
pub fn refresh_display(cell: &mut Cell) {
    cell.display = cell.value.display(cell.style.number_format.as_deref());
}
