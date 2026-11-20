//! `/lp verbose`: records the permission checks the server asks the handler and reports the ones a filter
//! lets through (`VerboseHandler`, `VerboseListener`, `VerboseFilter`). A check here is a `permission_handler_query`;
//! there is no stack trace to show, so the hover and the web viewer's trace stay empty.

use crate::horizon_luckperms_core::lang;
use crate::horizon_luckperms_core::runtime::{Recipient, Runtime};
use crate::horizon_luckperms_core::text::prefixed;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Most events a listener keeps for the upload (`DATA_TRUNCATION`).
const DATA_TRUNCATION: usize = 10_000;
/// Notifications per window (`NOTIFICATION_RATE_LIMIT_*`).
const RATE_PLAYER: usize = 50;
const RATE_CONSOLE: usize = 100;
const RATE_WINDOW_MS: i64 = 1000;

// ---- the filter expression (`BooleanExpressionCompiler`) ----------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ast {
    Always,
    Var(String),
    Not(Box<Ast>),
    And(Box<Ast>, Box<Ast>),
    Or(Box<Ast>, Box<Ast>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Var(String),
    Open,
    Close,
    And,
    Or,
    Not,
    Eof,
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut Vec<Tok>| {
        if !word.is_empty() {
            out.push(Tok::Var(std::mem::take(word)));
        }
    };
    for c in s.chars() {
        match c {
            '\u{0}'..='\u{20}' => flush(&mut word, &mut out),
            '(' | ')' | '&' | '|' | '!' => {
                flush(&mut word, &mut out);
                out.push(match c {
                    '(' => Tok::Open,
                    ')' => Tok::Close,
                    '&' => Tok::And,
                    '|' => Tok::Or,
                    _ => Tok::Not,
                });
            }
            '\u{21}'..='\u{7E}' => word.push(c),
            other => return Err(format!("Unknown token: {other}({})", other as u32)),
        }
    }
    flush(&mut word, &mut out);
    out.push(Tok::Eof);
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn cur(&self) -> &Tok {
        self.toks.get(self.pos).unwrap_or(&Tok::Eof)
    }
    fn exp(&mut self) -> Result<Ast, String> {
        let mut left = self.term()?;
        while *self.cur() == Tok::Or {
            self.pos += 1;
            let right = self.term()?;
            left = Ast::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }
    fn term(&mut self) -> Result<Ast, String> {
        let mut left = self.factor()?;
        while *self.cur() == Tok::And {
            self.pos += 1;
            let right = self.factor()?;
            left = Ast::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }
    fn factor(&mut self) -> Result<Ast, String> {
        match self.cur().clone() {
            Tok::Var(v) => {
                self.pos += 1;
                Ok(Ast::Var(v))
            }
            Tok::Not => {
                self.pos += 1;
                Ok(Ast::Not(Box::new(self.factor()?)))
            }
            Tok::Open => {
                self.pos += 1;
                let inner = self.exp()?;
                if *self.cur() != Tok::Close {
                    return Err("Brackets are not matched".into());
                }
                self.pos += 1;
                Ok(inner)
            }
            _ => Err("Malformed expression".into()),
        }
    }
}

/// `VerboseFilter.compile`: an empty expression accepts everything. The error is the reason `/lp verbose` shows.
pub fn compile(expression: &str) -> Result<Ast, String> {
    if expression.is_empty() {
        return Ok(Ast::Always);
    }
    let mut p = Parser { toks: lex(expression)?, pos: 0 };
    p.exp()
}

impl Ast {
    pub fn eval(&self, var: &dyn Fn(&str) -> bool) -> bool {
        match self {
            Ast::Always => true,
            Ast::Var(v) => var(v),
            Ast::Not(a) => !a.eval(var),
            Ast::And(a, b) => a.eval(var) && b.eval(var),
            Ast::Or(a, b) => a.eval(var) || b.eval(var),
        }
    }
}

// ---- events and listeners -------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Event {
    pub target: String,
    pub uuid: Option<u128>,
    pub permission: String,
    pub result: Option<bool>,
    pub time_ms: i64,
}

fn result_name(r: Option<bool>) -> &'static str {
    match r {
        Some(true) => "true",
        Some(false) => "false",
        None => "undefined",
    }
}

impl Event {
    /// `PermissionCheckEvent.eval`
    fn matches(&self, var: &str) -> bool {
        var == "permission"
            || self.target.eq_ignore_ascii_case(var)
            || self.permission.to_lowercase().starts_with(&var.to_lowercase())
            || result_name(self.result).eq_ignore_ascii_case(var)
    }

    fn json(&self) -> Value {
        let mut who = json!({ "identifier": self.target, "type": "user", "name": self.target });
        if let Some(u) = self.uuid {
            who["uuid"] = json!(crate::horizon_luckperms_core::uuid::format(u));
        }
        json!({
            "type": "permission",
            "origin": "platform_api_has_permission",
            "who": who,
            "queryMode": "contextual",
            "time": self.time_ms,
            "trace": [],
            "thread": "Server thread",
            "permission": self.permission,
            "result": result_name(self.result),
        })
    }
}

pub struct Listener {
    to: Recipient,
    sender_name: String,
    sender_uuid: u128,
    console: bool,
    filter: Ast,
    filter_text: String,
    notify: bool,
    start_ms: i64,
    counter: usize,
    matched: usize,
    results: Vec<Event>,
    stamps: Vec<i64>,
    warned: bool,
}

/// What `/lp verbose off|upload` takes back.
pub struct Finished {
    pub matched: usize,
    pub total: usize,
    pub filter_text: String,
    pub start_ms: i64,
    pub sender_name: String,
    pub sender_uuid: u128,
    pub results: Vec<Event>,
    pub truncated: bool,
}

static LISTENERS: Mutex<Option<HashMap<u128, Listener>>> = Mutex::new(None);
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

fn with<R>(f: impl FnOnce(&mut HashMap<u128, Listener>) -> R) -> R {
    let mut g = LISTENERS.lock().unwrap_or_else(|e| e.into_inner());
    let r = f(g.get_or_insert_with(HashMap::new));
    ACTIVE.store(g.as_ref().map_or(0, HashMap::len), Ordering::Relaxed);
    r
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// `registerListener`: a sender has one listener; a new one replaces the old.
pub fn register(to: Recipient, sender_uuid: u128, sender_name: &str, console: bool, filter: Ast, filter_text: &str, notify: bool) {
    with(|m| {
        m.insert(
            sender_uuid,
            Listener {
                to,
                sender_name: sender_name.to_owned(),
                sender_uuid,
                console,
                filter,
                filter_text: filter_text.to_owned(),
                notify,
                start_ms: now_ms(),
                counter: 0,
                matched: 0,
                results: Vec::new(),
                stamps: Vec::new(),
                warned: false,
            },
        );
    });
}

/// `unregisterListener`: None when the sender had none.
pub fn unregister(sender_uuid: u128) -> Option<Finished> {
    with(|m| m.remove(&sender_uuid)).map(|l| Finished {
        truncated: l.matched > l.results.len(),
        matched: l.matched,
        total: l.counter,
        filter_text: l.filter_text,
        start_ms: l.start_ms,
        sender_name: l.sender_name,
        sender_uuid: l.sender_uuid,
        results: l.results,
    })
}

/// Whether any listener runs (a cheap test for the permission hot path).
pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed) > 0
}

/// `VerboseHandler.offerPermissionCheckEvent`: one check, passed to every listener.
pub fn record(rt: &Runtime, uuid: u128, name: Option<String>, permission: &str, result: Option<bool>) {
    if !active() {
        return;
    }
    let event = Event {
        target: name.unwrap_or_else(|| crate::horizon_luckperms_core::uuid::format(uuid)),
        uuid: Some(uuid),
        permission: permission.to_owned(),
        result,
        time_ms: now_ms(),
    };
    let mut lines: Vec<(Recipient, String)> = Vec::new();
    with(|m| {
        for l in m.values_mut() {
            l.counter += 1;
            if !l.filter.eval(&|v| event.matches(v)) {
                continue;
            }
            l.matched += 1;
            if l.results.len() < DATA_TRUNCATION {
                l.results.push(event.clone());
            }
            if !l.notify {
                continue;
            }
            let now = now_ms();
            let max = if l.console { RATE_CONSOLE } else { RATE_PLAYER };
            l.stamps.retain(|t| now - *t <= RATE_WINDOW_MS);
            if l.stamps.len() >= max {
                if !l.warned {
                    l.warned = true;
                    lines.push((l.to.clone(), rate_limited()));
                }
                continue;
            }
            l.stamps.push(now);
            if l.stamps.len() < max {
                l.warned = false;
            }
            lines.push((l.to.clone(), notification(&event)));
        }
    });
    for (to, line) in lines {
        rt.push_out(to, line);
    }
}

/// `Message.VERBOSE_LOG_PERMISSION`
fn notification(e: &Event) -> String {
    let result = match e.result {
        Some(true) => "\u{a7}atrue",
        Some(false) => "\u{a7}cfalse",
        None => "\u{a7}7undefined",
    };
    prefixed(format!(
        "\u{a7}3{} \u{a7}3\u{a7}l> \u{a7}a{}\u{a7}7 - \u{a7}a{}\u{a7}7 - {result}",
        lang::T_LOGS_VERBOSE_PREFIX,
        e.target,
        e.permission
    ))
}

/// `Message.VERBOSE_NOTIFICATION_RATE_LIMITED`
fn rate_limited() -> String {
    let cmd = crate::horizon_luckperms_core::text::rich(
        "\u{a7}7/lp verbose upload",
        Some(crate::horizon_luckperms_core::text::Click::RunCommand("/lp verbose upload".into())),
        None,
    );
    prefixed(format!(
        "\u{a7}3{} \u{a7}3\u{a7}l> {}",
        lang::T_LOGS_VERBOSE_PREFIX,
        lang::tr(lang::T_LOGS_VERBOSE_RATE_LIMIT_EXCEEDED, 'c', &[cmd])
    ))
}

/// `yyyy-MM-dd HH:mm:ss UTC` of epoch milliseconds.
pub fn date_text(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // civil_from_days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// The JSON `uploadPasteData` posts.
pub fn upload_json(f: &Finished) -> String {
    let end = now_ms();
    let duration = crate::horizon_luckperms_core::duration::format_concise((end - f.start_ms) / 1000);
    let data: Vec<Value> = f.results.iter().map(Event::json).collect();
    let filter = if f.filter_text.is_empty() { "any".to_owned() } else { f.filter_text.clone() };
    json!({
        "metadata": {
            "startTime": date_text(f.start_ms),
            "endTime": date_text(end),
            "duration": duration,
            "count": { "matched": f.matched, "total": f.total },
            "uploader": { "name": f.sender_name, "uuid": crate::horizon_luckperms_core::uuid::format(f.sender_uuid) },
            "filter": filter,
            "truncated": f.truncated,
            "platform": "Horizon",
        },
        "data": data,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(target: &str, perm: &str, result: Option<bool>) -> Event {
        Event { target: target.into(), uuid: None, permission: perm.into(), result, time_ms: 0 }
    }

    fn passes(filter: &str, e: &Event) -> bool {
        compile(filter).unwrap().eval(&|v| e.matches(v))
    }

    #[test]
    fn an_empty_filter_accepts_everything() {
        assert_eq!(compile(""), Ok(Ast::Always));
        assert!(passes("", &event("Bob", "a.b", None)));
    }

    #[test]
    fn variables_match_the_user_a_permission_prefix_or_the_result() {
        let e = event("Bob", "essentials.fly", Some(true));
        assert!(passes("bob", &e));
        assert!(passes("essentials", &e));
        assert!(passes("true", &e));
        assert!(!passes("false", &e));
        assert!(!passes("alice", &e));
        assert!(passes("permission", &e));
    }

    #[test]
    fn operators_and_brackets() {
        let e = event("Bob", "a.b", Some(false));
        assert!(passes("bob & false", &e));
        assert!(!passes("bob & true", &e));
        assert!(passes("alice | a", &e));
        assert!(passes("!alice", &e));
        assert!(passes("(alice | bob) & !true", &e));
    }

    #[test]
    fn bad_expressions_are_errors() {
        assert_eq!(compile("(a"), Err("Brackets are not matched".into()));
        assert_eq!(compile("a &"), Err("Malformed expression".into()));
        assert_eq!(compile("&"), Err("Malformed expression".into()));
        assert!(compile("\u{e9}").is_err());
    }

    #[test]
    fn dates_are_written_in_utc() {
        assert_eq!(date_text(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(date_text(1_700_000_000_000), "2023-11-14 22:13:20 UTC");
    }
}
