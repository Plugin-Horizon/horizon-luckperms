//! UUIDs as the database stores them: lower-case, hyphenated, 36 characters.

/// The console's id in the action log (`00000000-0000-0000-0000-000000000000`).
pub const CONSOLE: u128 = 0;

/// `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`.
pub fn format(id: u128) -> String {
    let h = format!("{id:032x}");
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// Parses the hyphenated form, or 32 hex digits without hyphens.
pub fn parse(s: &str) -> Option<u128> {
    let s = s.trim();
    let hex: String = if s.len() == 36 {
        let b = s.as_bytes();
        if b[8] != b'-' || b[13] != b'-' || b[18] != b'-' || b[23] != b'-' {
            return None;
        }
        s.chars().filter(|c| *c != '-').collect()
    } else if s.len() == 32 {
        s.to_owned()
    } else {
        return None;
    };
    if hex.len() != 32 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u128::from_str_radix(&hex, 16).ok()
}

/// A random version 4 UUID (message ids of the messaging service).
pub fn random_v4() -> u128 {
    let mut b = [0u8; 16];
    if getrandom::fill(&mut b).is_err() {
        // Not expected; fall back on the clock so ids stay distinct.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        b = t.to_le_bytes();
    }
    let mut id = u128::from_be_bytes(b);
    id &= !(0xF000u128 << 64);
    id |= 0x4000u128 << 64; // version 4
    id &= !(0xC000_0000_0000_0000u128);
    id |= 0x8000_0000_0000_0000u128; // variant 10
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let id = 0x069a79f4_44e9_4726_a5be_fca90e38aaf5u128;
        let s = format(id);
        assert_eq!(s, "069a79f4-44e9-4726-a5be-fca90e38aaf5");
        assert_eq!(parse(&s), Some(id));
        assert_eq!(parse("069a79f444e94726a5befca90e38aaf5"), Some(id));
        assert_eq!(parse("nope"), None);
        assert_eq!(format(CONSOLE), "00000000-0000-0000-0000-000000000000");
    }

    #[test]
    fn v4_bits() {
        let id = random_v4();
        let s = format(id);
        assert_eq!(&s[14..15], "4");
        assert!(matches!(&s[19..20], "8" | "9" | "a" | "b"));
        assert_ne!(random_v4(), id);
    }
}
