//! The pieces `Message` builds its lines from, as legacy `§` strings: contexts, booleans, durations, track
//! paths, string lists. Each returns text that carries its own colour; `lang::tr` puts the parent colour back
//! after it.

use crate::context::ContextSet;
use crate::duration;
use crate::text::amp_to_section;

pub fn aq(s: &str) -> String {
    format!("\u{a7}b{s}")
}

pub fn dred(s: &str) -> String {
    format!("\u{a7}4{s}")
}

pub fn white(s: &str) -> String {
    format!("\u{a7}f{s}")
}

pub fn green(s: &str) -> String {
    format!("\u{a7}a{s}")
}

pub fn bool_aq(b: bool) -> String {
    aq(if b { "true" } else { "false" })
}

/// `formatBoolean`
pub fn bool_gr(b: bool) -> String {
    if b {
        green("true")
    } else {
        format!("\u{a7}cfalse")
    }
}

/// `formatTristate`
pub fn tristate(t: crate::calc::Tristate) -> String {
    match t {
        crate::calc::Tristate::True => green("true"),
        crate::calc::Tristate::False => "\u{a7}cfalse".into(),
        crate::calc::Tristate::Undefined => "\u{a7}7undefined".into(),
    }
}

/// `formatContextSet`: `&eglobal`, or `&3server=&bx&a, &3world=&by`.
pub fn ctx(c: &ContextSet) -> String {
    if c.is_empty() {
        return "\u{a7}eglobal".into();
    }
    let parts: Vec<String> = c.ordered_pairs().iter().map(|(k, v)| format!("\u{a7}3{k}=\u{a7}b{v}")).collect();
    format!("\u{a7}a{}", parts.join("\u{a7}a, "))
}

/// `formatContextSetBracketed`: `&8(&7server=&fx&8) &8(&7world=&fy&8)`, `if_empty` for none.
pub fn ctx_bracketed(c: &ContextSet, if_empty: &str) -> String {
    if c.is_empty() {
        return if_empty.to_owned();
    }
    c.ordered_pairs().iter().map(|(k, v)| format!("\u{a7}8(\u{a7}7{k}=\u{a7}f{v}\u{a7}8)")).collect::<Vec<_>>().join(" ")
}

/// `DurationFormatter.LONG` in aqua (`text().color(AQUA).append(...)`).
pub fn dur_aq(secs: i64) -> String {
    aq(&duration::format_long(secs))
}

/// `formatColoredValue`: `&` codes become colours. (MiniMessage values are shown as typed.)
pub fn colored_value(v: &str) -> String {
    if v.contains('&') || v.contains('\u{a7}') {
        amp_to_section(v)
    } else {
        v.to_owned()
    }
}

/// `formatStringList`
pub fn string_list(list: &[String]) -> String {
    if list.is_empty() {
        return "\u{a7}bNone".into();
    }
    format!("\u{a7}3{}", list.join("\u{a7}7, \u{a7}3"))
}

/// `formatTrackPath(groups)`
pub fn track_path(groups: &[String]) -> String {
    if groups.is_empty() {
        return "\u{a7}6None".into();
    }
    format!("\u{a7}3{}", groups.join("\u{a7}b ---> \u{a7}3"))
}

/// `formatTrackPath(groups, highlighted)`
pub fn track_path_hl(groups: &[String], highlighted: &str) -> String {
    if groups.is_empty() {
        return "\u{a7}bNone".into();
    }
    let mut out = String::from("\u{a7}3");
    for (i, g) in groups.iter().enumerate() {
        if i > 0 {
            out.push_str("\u{a7}7 ---> \u{a7}3");
        }
        if g.eq_ignore_ascii_case(highlighted) {
            out.push_str(&format!("\u{a7}b{g}\u{a7}3"));
        } else {
            out.push_str(g);
        }
    }
    out
}

/// `formatTrackPath(groups, first, second, reversed)`
pub fn track_path_progress(groups: &[String], first: &str, second: &str, reversed: bool) -> String {
    if groups.is_empty() {
        return "\u{a7}6None".into();
    }
    let mut out = String::from("\u{a7}3");
    let mut highlight = false;
    for (i, g) in groups.iter().enumerate() {
        if i > 0 {
            let arrow = if reversed { " <--- " } else { " ---> " };
            out.push_str(&format!("{}{arrow}\u{a7}3", if highlight { "\u{a7}4" } else { "\u{a7}7" }));
        }
        if g.eq_ignore_ascii_case(first) {
            out.push_str(&format!("\u{a7}b{g}\u{a7}3"));
            highlight = true;
        } else if g.eq_ignore_ascii_case(second) {
            out.push_str(&format!("\u{a7}b{g}\u{a7}3"));
            highlight = false;
        } else {
            out.push_str(g);
        }
    }
    out
}

/// `[bracketed] (page n of m - k entries)` as the headers write it: ` &7(page &fN&7 of &fM&7 - &fK&7 entries)`.
pub fn page_info(page: usize, pages: usize, entries: usize) -> String {
    format!(
        "\u{a7}7({} \u{a7}7- {}\u{a7}7)",
        crate::lang::tr(crate::lang::T_COMMAND_MISC_PAGE, '7', &[white(&page.to_string()), white(&pages.to_string())]),
        crate::lang::tr(crate::lang::T_COMMAND_MISC_PAGE_ENTRIES, '7', &[white(&entries.to_string())]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contexts() {
        assert_eq!(ctx(&ContextSet::new()), "\u{a7}eglobal");
        let c = ContextSet::of(&[("world", "w"), ("server", "s")]);
        assert_eq!(ctx(&c), "\u{a7}a\u{a7}3server=\u{a7}bs\u{a7}a, \u{a7}3world=\u{a7}bw");
        assert_eq!(ctx_bracketed(&ContextSet::new(), ""), "");
        assert_eq!(ctx_bracketed(&c, ""), "\u{a7}8(\u{a7}7server=\u{a7}fs\u{a7}8) \u{a7}8(\u{a7}7world=\u{a7}fw\u{a7}8)");
    }

    #[test]
    fn paths() {
        let g = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(crate::text::plain(&track_path(&g)), "a ---> b ---> c");
        assert_eq!(crate::text::plain(&track_path_progress(&g, "a", "b", false)), "a ---> b ---> c");
        assert_eq!(track_path(&[]), "\u{a7}6None");
    }

    #[test]
    fn page_header() {
        assert_eq!(crate::text::plain(&page_info(1, 2, 30)), "(page 1 of 2 - 30 entries)");
    }
}
