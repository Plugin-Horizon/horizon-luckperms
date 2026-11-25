//! The four file formats of LuckPerms' flat-file storages (Configurate's YAML, Gson JSON, HOCON and TOML
//! loaders), all read into and written from one ordered tree: the vendored YAML tree (`yaml::Node`), which
//! knows scalars with a type, lists and maps in file order.
//!
//! YAML and JSON use the parser and writer of the crate. HOCON is a reader for the subset of HOCON that
//! Configurate writes and people edit (paths, `=`/`:`/no separator, quoted and unquoted strings, `"""` blocks,
//! lists and objects separated by commas or lines, `#` and `//` comments; no substitutions and no includes) and
//! a writer in Typesafe Config's layout. TOML is read with the `toml` crate and written here, so that the order
//! of the keys stays and a list may hold strings and tables together.

use crate::horizon_luckperms_core::yaml::{self, Node as Tree};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Yaml,
    Json,
    Hocon,
    Toml,
}

impl Format {
    /// The file extension of the storage (`.yml`, `.json`, `.conf`, `.toml`).
    pub fn extension(self) -> &'static str {
        match self {
            Format::Yaml => ".yml",
            Format::Json => ".json",
            Format::Hocon => ".conf",
            Format::Toml => ".toml",
        }
    }

    /// The folder in the data directory (`yaml-storage`, ...). The combined variants use the same one.
    pub fn dir_name(self) -> &'static str {
        match self {
            Format::Yaml => "yaml-storage",
            Format::Json => "json-storage",
            Format::Hocon => "hocon-storage",
            Format::Toml => "toml-storage",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Yaml => "YAML",
            Format::Json => "JSON",
            Format::Hocon => "HOCON",
            Format::Toml => "TOML",
        }
    }

    /// Reads a file's text. Empty text is an empty map. The error names a line at most, never the text.
    pub fn parse(self, text: &str) -> Result<Tree, String> {
        if text.trim().is_empty() {
            return Ok(Tree::map());
        }
        match self {
            Format::Yaml => yaml::parse(text).map_err(|e| format!("parse error at line {}", e.line)),
            Format::Json => {
                let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("parse error at line {}", e.line()))?;
                Ok(from_json(&v))
            }
            Format::Hocon => parse_hocon(text),
            Format::Toml => {
                let t: toml::Table = toml::from_str(text).map_err(|_| "parse error".to_owned())?;
                Ok(from_toml(&toml::Value::Table(t)))
            }
        }
    }

    pub fn write(self, tree: &Tree) -> String {
        match self {
            Format::Yaml => yaml::write(tree),
            Format::Json => {
                let mut out = String::new();
                json_out(tree, 0, &mut out);
                out.push('\n');
                out
            }
            Format::Hocon => {
                let mut out = String::new();
                hocon_entries(tree, 0, &mut out);
                out
            }
            Format::Toml => toml_out(tree),
        }
    }
}

fn pad(out: &mut String, n: usize) {
    out.extend(std::iter::repeat_n(' ', n));
}

// ---- JSON ------------------------------------------------------------------------------------

fn from_json(v: &serde_json::Value) -> Tree {
    use serde_json::Value;
    match v {
        Value::Null => Tree::null(),
        Value::Bool(b) => Tree::bool(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => Tree::int(i),
            None => Tree::float(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => Tree::str(s),
        Value::Array(a) => Tree::seq(a.iter().map(from_json).collect()),
        Value::Object(m) => {
            let mut t = Tree::map();
            for (k, v) in m {
                t.set(k, from_json(v));
            }
            t
        }
    }
}

fn quote_json(s: &str) -> String {
    serde_json::Value::String(s.to_owned()).to_string()
}

/// A scalar as a typed literal: `Some(text)` for numbers and booleans, `None` for a string or null.
fn literal(n: &Tree) -> Option<String> {
    match n.tag() {
        "int" => n.as_i64().map(|i| i.to_string()),
        "float" => n.as_f64().filter(|f| f.is_finite()).map(|f| serde_json::Number::from_f64(f).map(|n| n.to_string()).unwrap_or_else(|| f.to_string())),
        "bool" => n.as_bool().map(|b| b.to_string()),
        _ => None,
    }
}

fn json_out(n: &Tree, ind: usize, out: &mut String) {
    match n {
        Tree::Scalar { value, .. } => {
            if n.is_null() {
                out.push_str("null");
            } else if let Some(l) = literal(n) {
                out.push_str(&l);
            } else {
                out.push_str(&quote_json(value));
            }
        }
        Tree::Seq { items, .. } => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                pad(out, ind + 2);
                json_out(item, ind + 2, out);
                out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
            }
            pad(out, ind);
            out.push(']');
        }
        Tree::Map { entries, .. } => {
            if entries.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            for (i, e) in entries.iter().enumerate() {
                pad(out, ind + 2);
                out.push_str(&quote_json(&e.key));
                out.push_str(": ");
                json_out(&e.value, ind + 2, out);
                out.push_str(if i + 1 < entries.len() { ",\n" } else { "\n" });
            }
            pad(out, ind);
            out.push('}');
        }
    }
}

// ---- TOML ------------------------------------------------------------------------------------

fn from_toml(v: &toml::Value) -> Tree {
    match v {
        toml::Value::String(s) => Tree::str(s),
        toml::Value::Integer(i) => Tree::int(*i),
        toml::Value::Float(f) => Tree::float(*f),
        toml::Value::Boolean(b) => Tree::bool(*b),
        toml::Value::Datetime(d) => Tree::str(&d.to_string()),
        toml::Value::Array(a) => Tree::seq(a.iter().map(from_toml).collect()),
        toml::Value::Table(t) => {
            let mut m = Tree::map();
            for (k, v) in t {
                m.set(k, from_toml(v));
            }
            m
        }
    }
}

fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn toml_key(k: &str) -> String {
    if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        k.to_owned()
    } else {
        toml_string(k)
    }
}

/// A value on one line (arrays and inline tables); null is left out by the callers.
fn toml_inline(n: &Tree, out: &mut String) {
    match n {
        Tree::Scalar { value, .. } => match literal(n) {
            Some(l) => out.push_str(&l),
            None => out.push_str(&toml_string(value)),
        },
        Tree::Seq { items, .. } => {
            out.push('[');
            let mut first = true;
            for item in items.iter().filter(|i| !i.is_null()) {
                if !first {
                    out.push_str(", ");
                }
                first = false;
                toml_inline(item, out);
            }
            out.push(']');
        }
        Tree::Map { entries, .. } => {
            out.push('{');
            let mut first = true;
            for e in entries.iter().filter(|e| !e.value.is_null()) {
                out.push_str(if first { " " } else { ", " });
                first = false;
                out.push_str(&toml_key(&e.key));
                out.push_str(" = ");
                toml_inline(&e.value, out);
            }
            out.push_str(if first { "}" } else { " }" });
        }
    }
}

/// Top-level values first, then one `[table]` per top-level map; everything deeper is inline. That is the shape of
/// both file kinds (one holder's file, or the combined file with one table per holder).
fn toml_out(root: &Tree) -> String {
    let mut out = String::new();
    for e in root.entries().iter().filter(|e| !e.value.is_null() && !e.value.is_map()) {
        out.push_str(&toml_key(&e.key));
        out.push_str(" = ");
        toml_inline(&e.value, &mut out);
        out.push('\n');
    }
    for e in root.entries().iter().filter(|e| e.value.is_map()) {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push('[');
        out.push_str(&toml_key(&e.key));
        out.push_str("]\n");
        for sub in e.value.entries().iter().filter(|s| !s.value.is_null()) {
            out.push_str(&toml_key(&sub.key));
            out.push_str(" = ");
            toml_inline(&sub.value, &mut out);
            out.push('\n');
        }
    }
    out
}

// ---- HOCON -----------------------------------------------------------------------------------

/// Plain letters, digits, `_` and `-` (not starting with a digit or `-`) are written bare, and not the words
/// HOCON reads as other types; everything else is quoted.
fn hocon_bare(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') && !matches!(s, "true" | "false" | "null")
}

fn hocon_str(s: &str) -> String {
    if hocon_bare(s) { s.to_owned() } else { quote_json(s) }
}

fn hocon_value(n: &Tree, ind: usize, out: &mut String) {
    match n {
        Tree::Scalar { value, .. } => {
            if n.is_null() {
                out.push_str("null");
            } else if let Some(l) = literal(n) {
                out.push_str(&l);
            } else {
                out.push_str(&hocon_str(value));
            }
        }
        Tree::Seq { items, .. } => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                pad(out, ind + 4);
                hocon_value(item, ind + 4, out);
                out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
            }
            pad(out, ind);
            out.push(']');
        }
        Tree::Map { entries, .. } => {
            if entries.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            hocon_entries(n, ind + 4, out);
            pad(out, ind);
            out.push('}');
        }
    }
}

fn hocon_entries(map: &Tree, ind: usize, out: &mut String) {
    for e in map.entries() {
        pad(out, ind);
        out.push_str(&hocon_str(&e.key));
        out.push('=');
        hocon_value(&e.value, ind, out);
        out.push('\n');
    }
}

struct Hocon {
    c: Vec<char>,
    i: usize,
}

fn herr(what: &str) -> String {
    format!("parse error: {what}")
}

impl Hocon {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn at(&self, s: &str) -> bool {
        s.chars().enumerate().all(|(k, ch)| self.c.get(self.i + k) == Some(&ch))
    }

    /// Spaces, tabs and (when `newlines`) line ends, and comments.
    fn skip(&mut self, newlines: bool) {
        loop {
            match self.peek() {
                Some(' ') | Some('\t') | Some('\r') | Some('\u{feff}') => self.i += 1,
                Some('\n') if newlines => self.i += 1,
                Some('#') => self.skip_line(),
                Some('/') if self.at("//") => self.skip_line(),
                _ => return,
            }
        }
    }

    fn skip_line(&mut self) {
        while let Some(c) = self.peek() {
            if c == '\n' {
                return;
            }
            self.i += 1;
        }
    }

    fn quoted(&mut self) -> Result<String, String> {
        if self.at("\"\"\"") {
            self.i += 3;
            let start = self.i;
            while self.i < self.c.len() && !self.at("\"\"\"") {
                self.i += 1;
            }
            if self.i >= self.c.len() {
                return Err(herr("unterminated block string"));
            }
            let mut end = self.i;
            self.i += 3;
            // Up to three more quotes belong to the text.
            while self.peek() == Some('"') {
                end += 1;
                self.i += 1;
            }
            return Ok(self.c[start..end].iter().collect());
        }
        self.i += 1;
        let mut s = String::new();
        loop {
            match self.peek() {
                None | Some('\n') => return Err(herr("unterminated string")),
                Some('"') => {
                    self.i += 1;
                    return Ok(s);
                }
                Some('\\') => {
                    self.i += 1;
                    match self.peek() {
                        Some('n') => s.push('\n'),
                        Some('t') => s.push('\t'),
                        Some('r') => s.push('\r'),
                        Some('b') => s.push('\u{8}'),
                        Some('f') => s.push('\u{c}'),
                        Some('"') => s.push('"'),
                        Some('\\') => s.push('\\'),
                        Some('/') => s.push('/'),
                        Some('u') => {
                            let hex: String = self.c.iter().skip(self.i + 1).take(4).collect();
                            let cp = u32::from_str_radix(&hex, 16).map_err(|_| herr("bad \\u escape"))?;
                            s.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                            self.i += 4;
                        }
                        _ => return Err(herr("bad escape")),
                    }
                    self.i += 1;
                }
                Some(c) => {
                    s.push(c);
                    self.i += 1;
                }
            }
        }
    }

    /// One key path: parts joined by `.`, each quoted or bare.
    fn path(&mut self) -> Result<Vec<String>, String> {
        let mut parts = Vec::new();
        loop {
            let part = if self.peek() == Some('"') {
                self.quoted()?
            } else {
                let start = self.i;
                while let Some(c) = self.peek() {
                    if c.is_whitespace() || matches!(c, '.' | '=' | ':' | '{' | '}' | '[' | ']' | ',' | '"' | '#' | '+') {
                        break;
                    }
                    self.i += 1;
                }
                if start == self.i {
                    return Err(herr("a key was expected"));
                }
                self.c[start..self.i].iter().collect()
            };
            parts.push(part);
            if self.peek() == Some('.') {
                self.i += 1;
            } else {
                return Ok(parts);
            }
        }
    }

    fn object_body(&mut self, close: Option<char>) -> Result<Tree, String> {
        let mut map = Tree::map();
        loop {
            self.skip(true);
            while self.peek() == Some(',') {
                self.i += 1;
                self.skip(true);
            }
            match (self.peek(), close) {
                (None, None) => return Ok(map),
                (None, Some(_)) => return Err(herr("an object is not closed")),
                (Some(c), Some(x)) if c == x => {
                    self.i += 1;
                    return Ok(map);
                }
                _ => {}
            }
            if self.at("include") && self.c.get(self.i + 7).is_some_and(|c| c.is_whitespace() || *c == '"') {
                return Err(herr("include is not supported"));
            }
            let path = self.path()?;
            self.skip(false);
            if self.at("+=") {
                return Err(herr("+= is not supported"));
            }
            match self.peek() {
                Some('=') | Some(':') => self.i += 1,
                Some('{') => {}
                _ => return Err(herr("a separator was expected")),
            }
            self.skip(true);
            let value = self.value()?;
            insert(&mut map, &path, value);
        }
    }

    fn array(&mut self) -> Result<Tree, String> {
        self.i += 1;
        let mut items = Vec::new();
        loop {
            self.skip(true);
            while self.peek() == Some(',') {
                self.i += 1;
                self.skip(true);
            }
            match self.peek() {
                None => return Err(herr("a list is not closed")),
                Some(']') => {
                    self.i += 1;
                    return Ok(Tree::seq(items));
                }
                _ => items.push(self.value()?),
            }
        }
    }

    fn value(&mut self) -> Result<Tree, String> {
        match self.peek() {
            Some('{') => {
                self.i += 1;
                self.object_body(Some('}'))
            }
            Some('[') => self.array(),
            Some('$') if self.at("${") => Err(herr("substitutions are not supported")),
            None => Err(herr("a value was expected")),
            _ => self.scalar(),
        }
    }

    /// A quoted or bare string, or several joined on the line (`a "b" c` is `a b c`); a lone bare word is typed.
    fn scalar(&mut self) -> Result<Tree, String> {
        let mut out = String::new();
        let mut pieces = 0;
        let mut only_bare = true;
        loop {
            // Whitespace between pieces is part of the text; at the end it is dropped.
            let ws_start = self.i;
            while matches!(self.peek(), Some(' ') | Some('\t')) {
                self.i += 1;
            }
            let ws: String = self.c[ws_start..self.i].iter().collect();
            match self.peek() {
                None | Some('\n') | Some('\r') | Some(',') | Some('}') | Some(']') | Some('#') => {
                    self.i = ws_start;
                    break;
                }
                Some('/') if self.at("//") => {
                    self.i = ws_start;
                    break;
                }
                Some('"') => {
                    if pieces > 0 {
                        out.push_str(&ws);
                    }
                    out.push_str(&self.quoted()?);
                    only_bare = false;
                }
                Some('$') if self.at("${") => return Err(herr("substitutions are not supported")),
                Some(_) => {
                    if pieces > 0 {
                        out.push_str(&ws);
                    }
                    let start = self.i;
                    while let Some(c) = self.peek() {
                        if matches!(c, '\n' | '\r' | ',' | '}' | ']' | '#' | '"' | ' ' | '\t') || (c == '/' && self.at("//")) {
                            break;
                        }
                        self.i += 1;
                    }
                    out.extend(self.c[start..self.i].iter());
                }
            }
            pieces += 1;
        }
        if pieces == 0 {
            return Err(herr("a value was expected"));
        }
        if only_bare {
            return Ok(typed(&out));
        }
        Ok(Tree::str(&out))
    }
}

/// A bare word read as HOCON reads it: `true`, `false`, `null`, a number, else text.
fn typed(s: &str) -> Tree {
    match s {
        "true" => return Tree::bool(true),
        "false" => return Tree::bool(false),
        "null" => return Tree::null(),
        _ => {}
    }
    let numeric = s.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '-' || c == '+' || c == '.')
        && s.chars().any(|c| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'));
    if numeric {
        if let Ok(i) = s.parse::<i64>() {
            return Tree::int(i);
        }
        if let Ok(f) = s.parse::<f64>() {
            return Tree::float(f);
        }
    }
    Tree::str(s)
}

/// Sets `path` in `map`, creating objects on the way; two objects at one key are merged, anything else is replaced.
fn insert(map: &mut Tree, path: &[String], value: Tree) {
    match path {
        [] => {}
        [last] => {
            if value.is_map() {
                if let Some(existing) = map.get_mut(last) {
                    if existing.is_map() {
                        for e in value.entries() {
                            existing.set(&e.key, e.value.clone());
                        }
                        return;
                    }
                }
            }
            map.set(last, value);
        }
        [first, rest @ ..] => {
            if map.get(first).is_none_or(|n| !n.is_map()) {
                map.set(first, Tree::map());
            }
            if let Some(child) = map.get_mut(first) {
                insert(child, rest, value);
            }
        }
    }
}

fn parse_hocon(text: &str) -> Result<Tree, String> {
    let mut p = Hocon { c: text.chars().collect(), i: 0 };
    p.skip(true);
    if p.peek() == Some('{') {
        p.i += 1;
        let t = p.object_body(Some('}'))?;
        p.skip(true);
        if p.peek().is_some() {
            return Err(herr("text after the root object"));
        }
        return Ok(t);
    }
    p.object_body(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Tree {
        let mut perm = Tree::map();
        perm.set("permission", Tree::str("essentials.fly"));
        perm.set("value", Tree::bool(false));
        let mut ctx = Tree::map();
        ctx.set("world", Tree::str("nether"));
        ctx.set("server", Tree::seq(vec![Tree::str("a"), Tree::str("b")]));
        perm.set("context", ctx);
        let mut root = Tree::map();
        root.set("uuid", Tree::str("069a79f4-44e9-4726-a5be-fca90e38aaf5"));
        root.set("name", Tree::str("Notch"));
        root.set("primary-group", Tree::str("default"));
        root.set("count", Tree::int(7));
        root.set("permissions", Tree::seq(vec![Tree::str("*"), Tree::str("a.b"), Tree::str("123"), Tree::str("true"), perm]));
        root.set("empty", Tree::seq(vec![]));
        root
    }

    /// A tree as a JSON value, so two trees compare by content and type, not by key order.
    fn norm(t: &Tree) -> serde_json::Value {
        serde_json::from_str(&Format::Json.write(t)).unwrap()
    }

    fn same(a: &Tree, b: &Tree) -> bool {
        norm(a) == norm(b)
    }

    #[test]
    fn every_format_round_trips_the_node_shapes() {
        for f in [Format::Yaml, Format::Json, Format::Hocon] {
            let text = f.write(&sample());
            let back = f.parse(&text).unwrap_or_else(|e| panic!("{f:?}: {e}\n{text}"));
            assert!(same(&sample(), &back), "{f:?}\n{text}\n{}", Format::Json.write(&back));
        }
    }

    #[test]
    fn toml_round_trips_a_holder_file_and_a_combined_file() {
        let mut root = sample();
        root.set("empty", Tree::seq(vec![]));
        let text = Format::Toml.write(&root);
        let back = Format::Toml.parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        // TOML has no key order: compare the values by key.
        for e in root.entries() {
            let b = back.get(&e.key).unwrap_or_else(|| panic!("missing {}\n{text}", e.key));
            assert_eq!(norm(&e.value), norm(b), "{}", e.key);
        }
        let mut combined = Tree::map();
        let mut one = Tree::map();
        one.set("name", Tree::str("x"));
        one.set("permissions", Tree::seq(vec![Tree::str("a.b")]));
        combined.set("069a79f4-44e9-4726-a5be-fca90e38aaf5", one.clone());
        combined.set("a.dotted group", one);
        let text = Format::Toml.write(&combined);
        let back = Format::Toml.parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(back.get("a.dotted group").and_then(|g| g.get("name")).and_then(Tree::as_str), Some("x"));
        assert_eq!(back.get("069a79f4-44e9-4726-a5be-fca90e38aaf5").map(|g| g.get("permissions").unwrap().items().len()), Some(1));
    }

    #[test]
    fn json_is_written_as_gson_does_with_two_spaces() {
        let mut t = Tree::map();
        t.set("name", Tree::str("default"));
        t.set("permissions", Tree::seq(vec![Tree::str("a")]));
        t.set("none", Tree::seq(vec![]));
        assert_eq!(Format::Json.write(&t), "{\n  \"name\": \"default\",\n  \"permissions\": [\n    \"a\"\n  ],\n  \"none\": []\n}\n");
    }

    #[test]
    fn hocon_reads_what_configurate_and_people_write() {
        let text = r#"
# a comment
uuid="069a79f4-44e9-4726-a5be-fca90e38aaf5"
name = Notch   // trailing
primary-group: default
permissions=[
    "essentials.fly",
    {
        permission="a.b"
        value=false
        context {
            world=nether
            server=[a, b]
        }
    }
    plain.node
]
parents=[{group=admin}]
nested.key.path = 5
nested { other = true }
"#;
        let t = Format::Hocon.parse(text).unwrap();
        assert_eq!(t.get("name").and_then(Tree::as_str), Some("Notch"));
        assert_eq!(t.get("primary-group").and_then(Tree::as_str), Some("default"));
        let perms = t.get("permissions").unwrap().items();
        assert_eq!(perms.len(), 3);
        assert_eq!(perms[0].as_str(), Some("essentials.fly"));
        assert_eq!(perms[1].get("value").and_then(Tree::as_bool), Some(false));
        assert_eq!(perms[1].get("context").and_then(|c| c.get("server")).map(|s| s.items().len()), Some(2));
        assert_eq!(perms[2].as_str(), Some("plain.node"));
        assert_eq!(t.get("parents").unwrap().items()[0].get("group").and_then(Tree::as_str), Some("admin"));
        assert_eq!(t.path("nested.key.path").and_then(Tree::as_i64), Some(5));
        assert_eq!(t.path("nested.other").and_then(Tree::as_bool), Some(true));
    }

    #[test]
    fn hocon_strings_keep_their_type_when_quoted() {
        let t = Format::Hocon.parse("a=\"123\"\nb=123\nc=\"true\"\nd=true\ne=\"\"\"two\nlines\"\"\"\nf=hello \"big\" world\n").unwrap();
        assert_eq!(t.get("a").map(Tree::tag), Some("str"));
        assert_eq!(t.get("b").map(Tree::tag), Some("int"));
        assert_eq!(t.get("c").map(Tree::tag), Some("str"));
        assert_eq!(t.get("d").map(Tree::tag), Some("bool"));
        assert_eq!(t.get("e").and_then(Tree::as_str), Some("two\nlines"));
        assert_eq!(t.get("f").and_then(Tree::as_str), Some("hello big world"));
    }

    #[test]
    fn hocon_refuses_what_it_does_not_read_and_never_echoes_text() {
        for bad in ["a=${b}\n", "include \"x.conf\"\n", "a=[1,2\n", "a=\"open\n", "a+=1\n", "{ a=1 } x\n"] {
            let e = Format::Hocon.parse(bad).unwrap_err();
            assert!(e.starts_with("parse error"), "{bad:?}: {e}");
        }
        let e = Format::Json.parse("{\"password\": secret-not-real,}").unwrap_err();
        assert!(!e.contains("secret"), "{e}");
    }

    #[test]
    fn empty_files_are_empty_maps() {
        for f in [Format::Yaml, Format::Json, Format::Hocon, Format::Toml] {
            let t = f.parse("  \n").unwrap();
            assert!(t.is_map() && t.entries().is_empty(), "{f:?}");
        }
    }
}
