//! Chat lines in legacy `§` codes; the Horizon glue turns them into components
//! (`Text::legacy`).

/// LuckPerms' chat prefix: `[LP]` in gray, the letters bold in aqua and dark aqua.
pub const PREFIX: &str = "\u{a7}7[\u{a7}b\u{a7}lL\u{a7}3\u{a7}lP\u{a7}7] ";

/// `Message.prefixed`: the prefix, a space, the message.
pub fn prefixed(s: impl AsRef<str>) -> String {
    format!("{PREFIX}{}", s.as_ref())
}

/// `&a` style codes (as prefixes are stored) to `§a`, for showing them coloured.
pub fn amp_to_section(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&' && i + 1 < chars.len() && "0123456789abcdefklmnorxABCDEFKLMNORX".contains(chars[i + 1]) {
            out.push('§');
            out.push(chars[i + 1].to_ascii_lowercase());
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// What a click on a rich segment does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Click {
    OpenUrl(String),
    SuggestCommand(String),
    RunCommand(String),
}

/// A piece of a line: legacy-coded text with an optional click and hover (legacy-coded too).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub click: Option<Click>,
    pub hover: Option<String>,
}

// A rich segment travels inside the line string, so every `Vec<String>` of lines stays as it was:
// START kind SEP value SEP hover SEP text END, with private-use characters.
const START: char = '\u{E000}';
const SEP: char = '\u{E001}';
const END: char = '\u{E002}';

/// A rich segment for a line: `text` (with `§` codes) that does `click` and shows `hover` on the mouse.
pub fn rich(text: &str, click: Option<Click>, hover: Option<&str>) -> String {
    let clean = |s: &str| s.chars().filter(|c| !matches!(*c, START | SEP | END)).collect::<String>();
    let (kind, value) = match &click {
        None => ('n', String::new()),
        Some(Click::OpenUrl(v)) => ('u', clean(v)),
        Some(Click::SuggestCommand(v)) => ('s', clean(v)),
        Some(Click::RunCommand(v)) => ('r', clean(v)),
    };
    format!("{START}{kind}{SEP}{value}{SEP}{}{SEP}{}{END}", clean(hover.unwrap_or("")), clean(text))
}

/// The line cut into plain and rich pieces; a line without rich segments is one piece.
pub fn segments(line: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(i) = rest.find(START) {
        let Some(len) = rest[i..].find(END) else { break };
        if i > 0 {
            out.push(Segment { text: rest[..i].to_owned(), click: None, hover: None });
        }
        let body = &rest[i + START.len_utf8()..i + len];
        let mut f = body.splitn(4, SEP);
        let (kind, value, hover, text) = (f.next().unwrap_or("n"), f.next().unwrap_or(""), f.next().unwrap_or(""), f.next().unwrap_or(""));
        let click = match kind {
            "u" => Some(Click::OpenUrl(value.to_owned())),
            "s" => Some(Click::SuggestCommand(value.to_owned())),
            "r" => Some(Click::RunCommand(value.to_owned())),
            _ => None,
        };
        out.push(Segment { text: text.to_owned(), click, hover: (!hover.is_empty()).then(|| hover.to_owned()) });
        rest = &rest[i + len + END.len_utf8()..];
    }
    if !rest.is_empty() || out.is_empty() {
        out.push(Segment { text: rest.to_owned(), click: None, hover: None });
    }
    out
}

/// Strips `§` codes and rich markup (for the console log and the tests).
pub fn plain(s: &str) -> String {
    let joined: String = if s.contains(START) { segments(s).into_iter().map(|g| g.text).collect() } else { s.to_owned() };
    let mut out = String::with_capacity(joined.len());
    let mut chars = joined.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

/// `QuotedStringTokenizer.tokenize(true)`: splits at single spaces (so two spaces make an empty argument); a
/// double or curly quote at the start of an argument runs to the next quote.
pub fn split_args(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let is_quote = |c: char| c == '"' || c == '\u{201C}' || c == '\u{201D}';
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if is_quote(chars[i]) {
            i += 1;
            let start = i;
            while i < chars.len() && !is_quote(chars[i]) {
                i += 1;
            }
            out.push(chars[start..i].iter().collect());
            if i < chars.len() {
                i += 1;
            }
            if i < chars.len() && chars[i] == ' ' {
                i += 1;
            }
        } else {
            let start = i;
            while i < chars.len() && chars[i] != ' ' {
                i += 1;
            }
            out.push(chars[start..i].iter().collect());
            if i < chars.len() {
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(amp_to_section("&c[Admin] &7"), "§c[Admin] §7");
        assert_eq!(amp_to_section("a & b"), "a & b");
        assert_eq!(plain("§c[Admin] §7x"), "[Admin] x");
    }

    #[test]
    fn rich_segments() {
        let line = format!("§7a {} b", rich("§bhttp://x/y", Some(Click::OpenUrl("http://x/y".into())), Some("§7Open")));
        assert_eq!(plain(&line), "a http://x/y b");
        let g = segments(&line);
        assert_eq!(g.len(), 3);
        assert_eq!(g[1].click, Some(Click::OpenUrl("http://x/y".into())));
        assert_eq!(g[1].hover.as_deref(), Some("§7Open"));
        assert_eq!(g[1].text, "§bhttp://x/y");
        assert_eq!(segments("plain")[0].click, None);
    }

    #[test]
    fn args() {
        assert_eq!(split_args(r#"user Bob meta addprefix 100 "&c[Admin] " server=lobby"#), vec!["user", "Bob", "meta", "addprefix", "100", "&c[Admin] ", "server=lobby"]);
        assert_eq!(split_args("a  b"), vec!["a", "", "b"]);
        assert_eq!(split_args(r#"x """#), vec!["x", ""]);
        assert!(split_args("").is_empty());
        assert_eq!(split_args("a b "), vec!["a", "b"]);
    }
}
