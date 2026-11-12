//! `ArgumentList`: the arguments after a command word, with LuckPerms' parsing of numbers, durations, temporary
//! modifiers and contexts, and the argument-count predicates (`Predicates`) commands use.

use crate::context::ContextSet;
use crate::duration::{self, DurationError};
use crate::model::TempMerge;

/// What went wrong with an argument; the command answers it as LuckPerms does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArgErr {
    /// `ArgumentException.DetailedUsage`: show the command's detailed usage.
    Usage,
    PastDate,
    InvalidDate(String),
    InvalidPriority(String),
}

/// The argument-count check of a command (`Command.getArgumentCheck`): true means "too few or too many".
#[derive(Clone, Copy, Debug)]
pub enum Check {
    AlwaysFalse,
    /// `Predicates.is(n)`
    Is(usize),
    /// `Predicates.not(n)`
    Not(usize),
    /// `Predicates.inRange(a, b)`
    InRange(usize, usize),
    /// `Predicates.notInRange(a, b)`
    NotInRange(usize, usize),
}

impl Check {
    pub fn test(self, n: usize) -> bool {
        match self {
            Check::AlwaysFalse => false,
            Check::Is(x) => n == x,
            Check::Not(x) => n != x,
            Check::InRange(a, b) => n >= a && n <= b,
            Check::NotInRange(a, b) => n < a || n > b,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Args {
    pub v: Vec<String>,
}

impl Args {
    pub fn new(v: Vec<String>) -> Self {
        Args { v }
    }

    pub fn len(&self) -> usize {
        self.v.len()
    }

    pub fn is_empty(&self) -> bool {
        self.v.is_empty()
    }

    /// `get(index)`: `{SPACE}` stands for a space.
    pub fn get(&self, i: usize) -> String {
        self.v.get(i).map(|s| s.replace("{SPACE}", " ")).unwrap_or_default()
    }

    pub fn get_or(&self, i: usize, default: &str) -> String {
        if i < self.v.len() {
            self.get(i)
        } else {
            default.to_owned()
        }
    }

    pub fn sub(&self, from: usize) -> Args {
        Args { v: self.v.iter().skip(from).cloned().collect() }
    }

    /// `getIntOrDefault`
    pub fn int_or(&self, i: usize, default: i64) -> i64 {
        if i >= self.v.len() {
            return default;
        }
        self.get(i).parse::<i32>().map(i64::from).unwrap_or(default)
    }

    /// `getBooleanOrInsert`: a `true`/`false` at `i` is taken; otherwise the default is inserted there.
    pub fn bool_or_insert(&mut self, i: usize, default: bool) -> bool {
        if i < self.v.len() {
            let a = self.get(i);
            if a.eq_ignore_ascii_case("true") || a.eq_ignore_ascii_case("false") {
                return a.eq_ignore_ascii_case("true");
            }
        }
        let at = i.min(self.v.len());
        self.v.insert(at, default.to_string());
        default
    }

    /// `getPriority`
    pub fn priority(&self, i: usize) -> Result<i32, ArgErr> {
        self.get(i).parse::<i32>().map_err(|_| ArgErr::InvalidPriority(self.get(i)))
    }

    /// `getDuration`: seconds.
    pub fn duration(&self, i: usize, now: i64) -> Result<i64, ArgErr> {
        let a = self.get(i);
        duration::parse_arg(&a, now).map_err(|e| match e {
            DurationError::Past => ArgErr::PastDate,
            DurationError::Invalid => ArgErr::InvalidDate(a),
        })
    }

    /// `getDurationOrDefault`: None when the argument is missing or no duration; a past date is still an error.
    pub fn duration_or_none(&self, i: usize, now: i64) -> Result<Option<i64>, ArgErr> {
        if i >= self.v.len() {
            return Ok(None);
        }
        match duration::parse_arg(&self.get(i), now) {
            Ok(d) => Ok(Some(d)),
            Err(DurationError::Past) => Err(ArgErr::PastDate),
            Err(DurationError::Invalid) => Ok(None),
        }
    }

    /// `getTemporaryModifierAndRemove`
    pub fn temp_modifier_and_remove(&mut self, i: usize) -> Option<TempMerge> {
        if i >= self.v.len() {
            return None;
        }
        let m = match self.get(i).to_lowercase().as_str() {
            "accumulate" => TempMerge::Accumulate,
            "replace" => TempMerge::Replace,
            "deny" | "none" => TempMerge::Deny,
            _ => return None,
        };
        self.v.remove(i);
        Some(m)
    }

    /// `getContextOrDefault` / `getContextOrEmpty`: the default contexts file is empty here.
    pub fn contexts(&self, from: usize) -> ContextSet {
        let mut set = ContextSet::new();
        for (i, entry) in self.v.iter().skip(from).enumerate() {
            let entry = entry.replace("{SPACE}", " ");
            let (key, value) = match entry.find('=') {
                Some(sep) => (entry[..sep].to_owned(), entry[sep + 1..].to_owned()),
                None => (if i == 1 { "world".to_owned() } else { "server".to_owned() }, entry.clone()),
            };
            if key.is_empty() || key.trim().is_empty() || value.is_empty() || value.trim().is_empty() {
                continue;
            }
            set.add(&key, &value);
        }
        set
    }

    /// `ArrayList.remove(Object)`: removes the first equal argument.
    pub fn remove_flag(&mut self, flag: &str) -> bool {
        match self.v.iter().position(|a| a == flag) {
            Some(i) => {
                self.v.remove(i);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &[&str]) -> Args {
        Args::new(s.iter().map(|x| x.to_string()).collect())
    }

    #[test]
    fn checks() {
        assert!(Check::Is(0).test(0));
        assert!(!Check::Is(0).test(1));
        assert!(Check::InRange(0, 1).test(1));
        assert!(!Check::InRange(0, 1).test(2));
        assert!(Check::NotInRange(1, 3).test(0));
        assert!(Check::NotInRange(1, 3).test(4));
        assert!(!Check::NotInRange(1, 3).test(2));
        assert!(Check::Not(1).test(2));
    }

    #[test]
    fn booleans_are_inserted() {
        let mut x = a(&["node", "server=x"]);
        assert!(x.bool_or_insert(1, true));
        assert_eq!(x.v, vec!["node", "true", "server=x"]);
        let mut y = a(&["node", "false"]);
        assert!(!y.bool_or_insert(1, true));
        assert_eq!(y.len(), 2);
    }

    #[test]
    fn contexts_second_bare_word_is_the_world() {
        let c = a(&["x", "survival", "nether"]).contexts(1);
        assert!(c.contains("server", "survival"));
        assert!(c.contains("world", "nether"));
        let c = a(&["x", "world=a", "b"]).contexts(1);
        assert!(c.contains("world", "a"));
        assert!(c.contains("world", "b"));
        assert!(a(&["x"]).contexts(1).is_empty());
    }

    #[test]
    fn durations_and_modifiers() {
        let mut x = a(&["n", "true", "1h", "accumulate", "server=x"]);
        assert_eq!(x.duration(2, 0), Ok(3600));
        assert_eq!(x.temp_modifier_and_remove(3), Some(TempMerge::Accumulate));
        assert_eq!(x.v.len(), 4);
        assert_eq!(a(&["30"]).duration(0, 1000), Err(ArgErr::PastDate));
        assert_eq!(a(&["zzz"]).duration(0, 0), Err(ArgErr::InvalidDate("zzz".into())));
        assert_eq!(a(&["zzz"]).duration_or_none(0, 0), Ok(None));
        assert_eq!(a(&["abc"]).priority(0), Err(ArgErr::InvalidPriority("abc".into())));
    }
}
