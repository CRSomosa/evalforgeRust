//! A minimal, hand-written JSON parser and writer.
//!
//! Why write one by hand instead of using serde?
//!   1. Zero dependencies — the whole project builds with std only.
//!   2. It's a great exercise: a recursive-descent parser is the classic
//!      way to turn text into structure, and JSON is small enough to do
//!      completely (strings with escapes, numbers, nesting, the lot).
//!
//! Supports the full JSON grammar. Objects preserve key order (we store
//! them as a Vec of pairs rather than a HashMap).

/// A parsed JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl Value {
    /// Walk a dotted path like "user.addresses.0.city".
    /// Object segments match keys; array segments are numeric indexes.
    pub fn get_path(&self, path: &str) -> Option<&Value> {
        let mut current = self;
        for seg in path.split('.') {
            match current {
                Value::Obj(pairs) => {
                    current = &pairs.iter().find(|(k, _)| k == seg)?.1;
                }
                Value::Arr(items) => {
                    let idx: usize = seg.parse().ok()?;
                    current = items.get(idx)?;
                }
                _ => return None,
            }
        }
        Some(current)
    }

    /// A human-comparable string form of a scalar value.
    /// Numbers that are whole print without a trailing ".0" so that
    /// `42` in JSON compares equal to the expectation string "42".
    pub fn as_comparable_string(&self) -> String {
        match self {
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Num(n) => format_number(*n),
            Value::Str(s) => s.clone(),
            // For containers, fall back to their JSON form.
            other => other.to_json(),
        }
    }

    /// Serialize back to compact JSON text.
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write_json(&mut out);
        out
    }

    fn write_json(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Num(n) => out.push_str(&format_number(*n)),
            Value::Str(s) => write_escaped_string(s, out),
            Value::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write_json(out);
                }
                out.push(']');
            }
            Value::Obj(pairs) => {
                out.push('{');
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_escaped_string(k, out);
                    out.push(':');
                    v.write_json(out);
                }
                out.push('}');
            }
        }
    }
}

/// Whole numbers print as integers ("42"), everything else uses
/// Rust's shortest-roundtrip float formatting.
fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

fn write_escaped_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Other control characters must be \u escaped per the spec.
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Parse a complete JSON document. Trailing garbage is an error.
pub fn parse(input: &str) -> Result<Value, String> {
    let mut p = Parser {
        chars: input.chars().collect(),
        pos: 0,
    };
    p.skip_whitespace();
    let value = p.parse_value()?;
    p.skip_whitespace();
    if p.pos != p.chars.len() {
        return Err(format!(
            "unexpected trailing characters at position {}",
            p.pos
        ));
    }
    Ok(value)
}

/// A classic recursive-descent parser: hold the input and a cursor,
/// and write one small method per grammar rule.
struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c == ' ' || c == '\t' || c == '\n' || c == '\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), String> {
        match self.advance() {
            Some(c) if c == expected => Ok(()),
            Some(c) => Err(format!(
                "expected '{}' but found '{}' at position {}",
                expected,
                c,
                self.pos - 1
            )),
            None => Err(format!("expected '{}' but input ended", expected)),
        }
    }

    /// Dispatch on the first character to decide which rule applies.
    fn parse_value(&mut self) -> Result<Value, String> {
        self.skip_whitespace();
        match self.peek() {
            Some('{') => self.parse_object(),
            Some('[') => self.parse_array(),
            Some('"') => Ok(Value::Str(self.parse_string()?)),
            Some('t') => self.parse_keyword("true", Value::Bool(true)),
            Some('f') => self.parse_keyword("false", Value::Bool(false)),
            Some('n') => self.parse_keyword("null", Value::Null),
            Some(c) if c == '-' || c.is_ascii_digit() => self.parse_number(),
            Some(c) => Err(format!(
                "unexpected character '{}' at position {}",
                c, self.pos
            )),
            None => Err("unexpected end of input".to_string()),
        }
    }

    fn parse_keyword(&mut self, word: &str, value: Value) -> Result<Value, String> {
        for expected in word.chars() {
            match self.advance() {
                Some(c) if c == expected => {}
                _ => return Err(format!("invalid literal, expected `{}`", word)),
            }
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        // Collect every character that can appear in a JSON number and
        // let Rust's f64 parser validate the final shape.
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        text.parse::<f64>()
            .map(Value::Num)
            .map_err(|_| format!("invalid number `{}` at position {}", text, start))
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            match self.advance() {
                None => return Err("unterminated string".to_string()),
                Some('"') => return Ok(out),
                Some('\\') => match self.advance() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('/') => out.push('/'),
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('b') => out.push('\u{0008}'),
                    Some('f') => out.push('\u{000C}'),
                    Some('u') => {
                        // \uXXXX — four hex digits.
                        let mut code = 0u32;
                        for _ in 0..4 {
                            let c = self
                                .advance()
                                .ok_or_else(|| "unterminated \\u escape".to_string())?;
                            let digit = c
                                .to_digit(16)
                                .ok_or_else(|| format!("invalid hex digit '{}'", c))?;
                            code = code * 16 + digit;
                        }
                        // Note: surrogate pairs (emoji etc.) are not
                        // recombined — out of scope for this harness.
                        match char::from_u32(code) {
                            Some(c) => out.push(c),
                            None => out.push('\u{FFFD}'), // replacement char
                        }
                    }
                    Some(c) => return Err(format!("invalid escape '\\{}'", c)),
                    None => return Err("unterminated escape".to_string()),
                },
                Some(c) => out.push(c),
            }
        }
    }

    fn parse_array(&mut self) -> Result<Value, String> {
        self.expect('[')?;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Ok(Value::Arr(items));
        }
        loop {
            items.push(self.parse_value()?);
            self.skip_whitespace();
            match self.advance() {
                Some(',') => continue,
                Some(']') => return Ok(Value::Arr(items)),
                Some(c) => return Err(format!("expected ',' or ']' but found '{}'", c)),
                None => return Err("unterminated array".to_string()),
            }
        }
    }

    fn parse_object(&mut self) -> Result<Value, String> {
        self.expect('{')?;
        let mut pairs = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(Value::Obj(pairs));
        }
        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            self.skip_whitespace();
            self.expect(':')?;
            let value = self.parse_value()?;
            pairs.push((key, value));
            self.skip_whitespace();
            match self.advance() {
                Some(',') => continue,
                Some('}') => return Ok(Value::Obj(pairs)),
                Some(c) => return Err(format!("expected ',' or '}}' but found '{}'", c)),
                None => return Err("unterminated object".to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scalars() {
        assert_eq!(parse("null").unwrap(), Value::Null);
        assert_eq!(parse("true").unwrap(), Value::Bool(true));
        assert_eq!(parse("false").unwrap(), Value::Bool(false));
        assert_eq!(parse("42").unwrap(), Value::Num(42.0));
        assert_eq!(parse("-3.5e2").unwrap(), Value::Num(-350.0));
        assert_eq!(parse("\"hi\"").unwrap(), Value::Str("hi".to_string()));
    }

    #[test]
    fn parses_string_escapes() {
        assert_eq!(
            parse(r#""a\nb\t\"c\" A""#).unwrap(),
            Value::Str("a\nb\t\"c\" A".to_string())
        );
    }

    #[test]
    fn parses_nested_structures() {
        let v = parse(r#"{"name": "Ada", "tags": ["math", "code"], "age": 36}"#).unwrap();
        assert_eq!(
            v.get_path("name").unwrap(),
            &Value::Str("Ada".to_string())
        );
        assert_eq!(
            v.get_path("tags.1").unwrap(),
            &Value::Str("code".to_string())
        );
        assert_eq!(v.get_path("age").unwrap(), &Value::Num(36.0));
        assert!(v.get_path("missing").is_none());
        assert!(v.get_path("tags.9").is_none());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("").is_err());
        assert!(parse("{").is_err());
        assert!(parse("[1, 2,]").is_err());
        assert!(parse("\"unterminated").is_err());
        assert!(parse("42 extra").is_err());
    }

    #[test]
    fn roundtrips_to_json() {
        let text = r#"{"a":[1,2,{"b":"x\ny"}],"c":null,"d":true}"#;
        let v = parse(text).unwrap();
        assert_eq!(v.to_json(), text);
    }

    #[test]
    fn comparable_strings() {
        assert_eq!(parse("42").unwrap().as_comparable_string(), "42");
        assert_eq!(parse("42.5").unwrap().as_comparable_string(), "42.5");
        assert_eq!(parse("\"x\"").unwrap().as_comparable_string(), "x");
        assert_eq!(parse("true").unwrap().as_comparable_string(), "true");
    }
}
