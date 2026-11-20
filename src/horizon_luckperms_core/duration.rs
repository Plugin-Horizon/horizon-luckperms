//! Durations of temporary nodes, as `/lp` reads and prints them: `1y2mo3w4d5h6m7s`,
//! `30m`, `2 days`, or a unix timestamp in seconds for an absolute expiry.

const UNITS: [(&str, u64); 7] = [
    ("y", 365 * 86_400),
    ("mo", 30 * 86_400),
    ("w", 7 * 86_400),
    ("d", 86_400),
    ("h", 3_600),
    ("m", 60),
    ("s", 1),
];

/// Seconds since the unix epoch.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The expiry (unix seconds) an argument stands for, from `now`: a duration from now, or an
/// absolute timestamp that lies in the future. None when it is neither.
pub fn parse_expiry(arg: &str, now: i64) -> Option<i64> {
    let arg = arg.trim();
    if let Ok(ts) = arg.parse::<i64>() {
        // A plain number: an absolute timestamp when in the future, else seconds.
        return Some(if ts > now { ts } else { now + ts.max(1) });
    }
    parse_duration(arg).map(|d| now + d as i64)
}

/// A duration in seconds; units may be spelt out (`2days`, `3 hours`). None when nothing parses.
pub fn parse_duration(arg: &str) -> Option<u64> {
    let s = arg.to_lowercase();
    let b = s.as_bytes();
    let mut i = 0;
    let mut total: u64 = 0;
    let mut any = false;
    while i < b.len() {
        while i < b.len() && (b[i] == b' ' || b[i] == b',') {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if start == i {
            return None;
        }
        let n: u64 = s[start..i].parse().ok()?;
        while i < b.len() && b[i] == b' ' {
            i += 1;
        }
        let ustart = i;
        while i < b.len() && b[i].is_ascii_alphabetic() {
            i += 1;
        }
        let unit = &s[ustart..i];
        let secs = if unit.is_empty() {
            1
        } else if unit.starts_with("mo") {
            30 * 86_400
        } else {
            let first = &unit[..1];
            UNITS.iter().find(|(u, _)| *u == first)?.1
        };
        total = total.checked_add(n.checked_mul(secs)?)?;
        any = true;
    }
    any.then_some(total)
}

/// `1d 2h 3m 4s`; `0s` for nothing.
pub fn format(mut secs: u64) -> String {
    if secs == 0 {
        return "0s".into();
    }
    let mut parts = Vec::new();
    for (u, n) in UNITS {
        if secs >= n {
            parts.push(format!("{}{}", secs / n, u));
            secs %= n;
        }
    }
    parts.join(" ")
}

/// The time left until `expiry`, formatted; `0s` once passed.
pub fn format_left(expiry: i64, now: i64) -> String {
    format((expiry - now).max(0) as u64)
}

/// Seconds of `ChronoUnit` YEARS, MONTHS, WEEKS, DAYS, HOURS, MINUTES, SECONDS (what LuckPerms'
/// `DurationFormatter` and `DurationParser` use).
const LP_UNIT_SECS: [i64; 7] = [31_556_952, 2_629_746, 604_800, 86_400, 3_600, 60, 1];
const LP_UNIT_LONG: [(&str, &str); 7] =
    [("year", "years"), ("month", "months"), ("week", "weeks"), ("day", "days"), ("hour", "hours"), ("minute", "minutes"), ("second", "seconds")];
const LP_UNIT_SHORT: [&str; 7] = ["y", "mo", "w", "d", "h", "m", "s"];

fn lp_format(secs: i64, concise: bool) -> String {
    let mut seconds = secs.max(0);
    let mut parts: Vec<String> = Vec::new();
    for (i, unit) in LP_UNIT_SECS.iter().enumerate() {
        let n = seconds / unit;
        if n > 0 {
            seconds -= unit * n;
            parts.push(lp_part(n, i, concise));
        }
        if seconds <= 0 {
            break;
        }
    }
    if parts.is_empty() {
        return lp_part(0, 6, concise);
    }
    parts.join(" ")
}

fn lp_part(n: i64, unit: usize, concise: bool) -> String {
    if concise {
        format!("{n}{}", LP_UNIT_SHORT[unit])
    } else if n == 1 {
        format!("{n} {}", LP_UNIT_LONG[unit].0)
    } else {
        format!("{n} {}", LP_UNIT_LONG[unit].1)
    }
}

/// `DurationFormatter.LONG`: `1 day 2 hours`.
pub fn format_long(secs: i64) -> String {
    lp_format(secs, false)
}

/// `DurationFormatter.CONCISE`: `1d 2h`.
pub fn format_concise(secs: i64) -> String {
    lp_format(secs, true)
}

/// Why a duration argument was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DurationError {
    /// A timestamp in the past.
    Past,
    /// Neither a timestamp nor a duration.
    Invalid,
}

/// `ArgumentList.parseDuration`: a plain number is a unix timestamp (the duration until it), anything
/// else a `DurationParser` string (`1y 2mo 3w 4d 5h 6m 7s`, the units in this order, each optional).
pub fn parse_arg(input: &str, now: i64) -> Result<i64, DurationError> {
    if let Ok(number) = input.parse::<i64>() {
        let d = number - now;
        return if d < 0 { Err(DurationError::Past) } else { Ok(d) };
    }
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = PATTERN.get_or_init(|| {
        regex::Regex::new(concat!(
            r"(?i)^\s*(?:(\d+)\s*y(?:ear)?s?[,\s]*)?(?:(\d+)\s*mo(?:nth)?s?[,\s]*)?(?:(\d+)\s*w(?:eek)?s?[,\s]*)?",
            r"(?:(\d+)\s*d(?:ay)?s?[,\s]*)?(?:(\d+)\s*h(?:our|r)?s?[,\s]*)?(?:(\d+)\s*m(?:inute|in)?s?[,\s]*)?",
            r"(?:(\d+)\s*s(?:econd|ec)?s?[,\s]*)?$"
        ))
        .expect("duration pattern")
    });
    let caps = re.captures(input).ok_or(DurationError::Invalid)?;
    let mut total: i64 = 0;
    for (i, unit) in LP_UNIT_SECS.iter().enumerate() {
        if let Some(m) = caps.get(i + 1) {
            let n: i64 = m.as_str().parse().map_err(|_| DurationError::Invalid)?;
            if n > 0 {
                total = total.saturating_add(unit.saturating_mul(n));
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        assert_eq!(parse_duration("1d"), Some(86_400));
        assert_eq!(parse_duration("1h30m"), Some(5_400));
        assert_eq!(parse_duration("2 days 3 hours"), Some(2 * 86_400 + 3 * 3_600));
        assert_eq!(parse_duration("1mo"), Some(30 * 86_400));
        assert_eq!(parse_duration("1w1s"), Some(7 * 86_400 + 1));
        assert_eq!(parse_duration("abc"), None);
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("5x"), None);
    }

    #[test]
    fn expiry() {
        assert_eq!(parse_expiry("1h", 1000), Some(4600));
        assert_eq!(parse_expiry("5000", 1000), Some(5000));
        assert_eq!(parse_expiry("60", 1000), Some(1060));
    }

    #[test]
    fn formats() {
        assert_eq!(format(90_061), "1d 1h 1m 1s");
        assert_eq!(format(0), "0s");
        assert_eq!(format_left(10, 20), "0s");
    }

    #[test]
    fn luckperms_formats() {
        assert_eq!(format_long(90_061), "1 day 1 hour 1 minute 1 second");
        assert_eq!(format_long(7_200), "2 hours");
        assert_eq!(format_concise(90_061), "1d 1h 1m 1s");
        assert_eq!(format_long(0), "0 seconds");
        assert_eq!(format_concise(-5), "0s");
    }

    #[test]
    fn luckperms_parse_arg() {
        assert_eq!(parse_arg("1h30m", 0), Ok(5_400));
        assert_eq!(parse_arg("2 days, 3 hours", 0), Ok(2 * 86_400 + 3 * 3_600));
        assert_eq!(parse_arg("1mo", 0), Ok(2_629_746));
        assert_eq!(parse_arg("abc", 0), Err(DurationError::Invalid));
        assert_eq!(parse_arg("5x", 0), Err(DurationError::Invalid));
        assert_eq!(parse_arg("30", 1_000), Err(DurationError::Past));
        assert_eq!(parse_arg("5000", 1_000), Ok(4_000));
        assert_eq!(parse_arg("s1", 0), Err(DurationError::Invalid));
    }
}
