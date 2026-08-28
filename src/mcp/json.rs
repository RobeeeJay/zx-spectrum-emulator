//! Just enough JSON for the MCP server to speak it.
//!
//! There is no JSON crate in the lock file and none can be added, so this is
//! the same bargain as `src/svg.rs`: a small implementation of exactly what is
//! used, rather than a general one. What is used is JSON-RPC 2.0 — objects,
//! arrays, strings, numbers, booleans and null — and the numbers are all
//! integers of a size a Spectrum can hold.

use std::fmt::Write as _;

/// A JSON value. Objects keep the order they were written in, which is what
/// makes the server's replies diffable between runs.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// An object, built from pairs.
    pub fn obj<const N: usize>(fields: [(&str, Json); N]) -> Json {
        Json::Obj(
            fields
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }

    pub fn str(text: impl Into<String>) -> Json {
        Json::Str(text.into())
    }

    pub fn num(value: impl Into<f64>) -> Json {
        Json::Num(value.into())
    }

    pub fn arr(items: Vec<Json>) -> Json {
        Json::Arr(items)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    /// A whole number, however it was written. `0x8000` is not JSON, so an
    /// address may arrive as a number or as a string like "8000" or "$8000";
    /// `as_addr` is where that is untangled, not here.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Num(n) if n.fract() == 0.0 => Some(*n as i64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }
}

impl std::fmt::Display for Json {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Json::Null => f.write_str("null"),
            Json::Bool(true) => f.write_str("true"),
            Json::Bool(false) => f.write_str("false"),
            Json::Num(n) => {
                if n.fract() == 0.0 && n.is_finite() && n.abs() < 9e15 {
                    write!(f, "{}", *n as i64)
                } else {
                    write!(f, "{n}")
                }
            }
            Json::Str(s) => write_string(f, s),
            Json::Arr(items) => {
                f.write_str("[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
            Json::Obj(fields) => {
                f.write_str("{")?;
                for (i, (key, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write_string(f, key)?;
                    f.write_str(":")?;
                    write!(f, "{value}")?;
                }
                f.write_str("}")
            }
        }
    }
}

fn write_string(f: &mut std::fmt::Formatter<'_>, s: &str) -> std::fmt::Result {
    f.write_str("\"")?;
    for c in s.chars() {
        match c {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            // A control character has to be escaped or the message is not
            // JSON; everything else goes out as itself, UTF-8 and all.
            c if (c as u32) < 0x20 => {
                let mut buf = String::new();
                let _ = write!(buf, "\\u{:04x}", c as u32);
                f.write_str(&buf)?
            }
            c => f.write_char(c)?,
        }
    }
    f.write_str("\"")
}

/// Read one JSON value from `text`. Trailing content is an error: a JSON-RPC
/// message is one value and a line holding two is a line to complain about.
pub fn parse(text: &str) -> Result<Json, String> {
    let mut p = Parser {
        chars: text.chars().collect(),
        at: 0,
    };
    p.space();
    let value = p.value()?;
    p.space();
    if p.at < p.chars.len() {
        return Err(format!("trailing text at character {}", p.at));
    }
    Ok(value)
}

struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.at += 1;
        }
        c
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_ascii_whitespace()) {
            self.at += 1;
        }
    }

    fn expect(&mut self, want: char) -> Result<(), String> {
        match self.next() {
            Some(c) if c == want => Ok(()),
            Some(c) => Err(format!(
                "expected {want:?} at character {}, got {c:?}",
                self.at
            )),
            None => Err(format!("expected {want:?}, and the text ended")),
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Result<Json, String> {
        for want in word.chars() {
            self.expect(want)?;
        }
        Ok(value)
    }

    fn value(&mut self) -> Result<Json, String> {
        match self.peek() {
            None => Err("the text ended where a value should be".into()),
            Some('{') => self.object(),
            Some('[') => self.array(),
            Some('"') => Ok(Json::Str(self.string()?)),
            Some('t') => self.word("true", Json::Bool(true)),
            Some('f') => self.word("false", Json::Bool(false)),
            Some('n') => self.word("null", Json::Null),
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            Some(c) => Err(format!("{c:?} does not start a value")),
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.expect('{')?;
        let mut fields = Vec::new();
        self.space();
        if self.peek() == Some('}') {
            self.at += 1;
            return Ok(Json::Obj(fields));
        }
        loop {
            self.space();
            let key = self.string()?;
            self.space();
            self.expect(':')?;
            self.space();
            let value = self.value()?;
            fields.push((key, value));
            self.space();
            match self.next() {
                Some(',') => continue,
                Some('}') => return Ok(Json::Obj(fields)),
                other => return Err(format!("expected ',' or '}}', got {other:?}")),
            }
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.expect('[')?;
        let mut items = Vec::new();
        self.space();
        if self.peek() == Some(']') {
            self.at += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            self.space();
            items.push(self.value()?);
            self.space();
            match self.next() {
                Some(',') => continue,
                Some(']') => return Ok(Json::Arr(items)),
                other => return Err(format!("expected ',' or ']', got {other:?}")),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            match self.next() {
                None => return Err("the text ended inside a string".into()),
                Some('"') => return Ok(out),
                Some('\\') => match self.next() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('/') => out.push('/'),
                    Some('b') => out.push('\u{8}'),
                    Some('f') => out.push('\u{c}'),
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('u') => out.push(self.unicode()?),
                    other => return Err(format!("{other:?} is not an escape")),
                },
                Some(c) => out.push(c),
            }
        }
    }

    /// A `\uXXXX` escape, and the second half of a surrogate pair if this was
    /// the first: a character outside the basic plane is written as two.
    fn unicode(&mut self) -> Result<char, String> {
        let first = self.hex4()?;
        if !(0xD800..0xDC00).contains(&first) {
            return char::from_u32(first)
                .ok_or_else(|| format!("\\u{first:04x} is not a character"));
        }
        self.expect('\\')?;
        self.expect('u')?;
        let second = self.hex4()?;
        if !(0xDC00..0xE000).contains(&second) {
            return Err(format!("\\u{second:04x} does not follow a surrogate"));
        }
        let combined = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
        char::from_u32(combined).ok_or_else(|| format!("\\u{combined:04x} is not a character"))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let mut value = 0u32;
        for _ in 0..4 {
            let c = self.next().ok_or("the text ended inside a \\u escape")?;
            let digit = c
                .to_digit(16)
                .ok_or_else(|| format!("{c:?} is not a hex digit"))?;
            value = value * 16 + digit;
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.at;
        if self.peek() == Some('-') {
            self.at += 1;
        }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.at += 1;
        }
        if self.peek() == Some('.') {
            self.at += 1;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.at += 1;
            }
        }
        if matches!(self.peek(), Some('e') | Some('E')) {
            self.at += 1;
            if matches!(self.peek(), Some('+') | Some('-')) {
                self.at += 1;
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.at += 1;
            }
        }
        let text: String = self.chars[start..self.at].iter().collect();
        text.parse::<f64>()
            .map(Json::Num)
            .map_err(|_| format!("{text:?} is not a number"))
    }
}
