//! Translations (`TranslationManager`, `TranslationRepository`): `translations/<locale>.properties` (custom, wins) and
//! `translations/repo/<locale>.properties` (installed by `/lp translations install`) replace the English message
//! templates of [`lang`](crate::horizon_luckperms_core::lang) for the server locale (`horizon.locale`).
//! A file is matched by key against the shipped `luckperms_en.properties`; the English value is the lookup key.

use crate::horizon_luckperms_core::bytebin::{Bytebin, HttpError};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

const EN: &str = include_str!("luckperms_en.properties");
const METADATA: &str = "https://metadata.luckperms.net/";
const INFO_PATH: &str = "data/translations";
const DOWNLOAD_PATH: &str = "translation/";
/// Never more than this many languages (`TranslationRepository`: a silly metadata server).
const MAX_LANGUAGES: usize = 100;

static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
static MAP: RwLock<Option<HashMap<String, String>>> = RwLock::new(None);

/// One language the repository offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageInfo {
    pub id: String,
    pub name: String,
    pub locale: String,
    pub progress: i64,
    pub contributors: Vec<String>,
}

/// `PropertyResourceBundle` in the small: `key=value` lines, `#`/`!` comments, `\n`, `\t`, `\uXXXX` and `\\`;
/// the value's `''` (MessageFormat) is one apostrophe.
pub fn parse_properties(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        let Some(sep) = line.find(['=', ':']) else { continue };
        let key = line[..sep].trim().to_owned();
        out.insert(key, unescape(line[sep + 1..].trim_start()).replace("''", "'"));
    }
    out
}

fn unescape(v: &str) -> String {
    let mut out = String::new();
    let mut it = v.chars();
    while let Some(ch) = it.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let hex: String = it.by_ref().take(4).collect();
                if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(c);
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// `en` -> English, the default: no file is read.
fn is_english(locale: &str) -> bool {
    let l = locale.to_lowercase().replace('-', "_");
    l.is_empty() || l == "en" || l.starts_with("en_")
}

/// The candidates for a locale tag: `de_DE`, then `de` (Adventure's fallback).
fn candidates(locale: &str) -> Vec<String> {
    let l = locale.replace('-', "_");
    let mut v = vec![l.clone()];
    if let Some(i) = l.find('_') {
        v.push(l[..i].to_owned());
    }
    v
}

fn read_locale(dir: &Path, locale: &str) -> HashMap<String, String> {
    for c in candidates(locale) {
        for name in [format!("{c}.properties"), format!("messages_{c}.properties")] {
            if let Ok(text) = std::fs::read_to_string(dir.join(&name)) {
                return parse_properties(&text);
            }
        }
    }
    HashMap::new()
}

/// The English template -> translated template map for `locale`: a custom file wins over the repository's.
pub fn build_map(base: &Path, locale: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if is_english(locale) {
        return out;
    }
    let english = parse_properties(EN);
    let mut translated = read_locale(&base.join("repo"), locale);
    translated.extend(read_locale(base, locale));
    for (key, value) in translated {
        if let Some(en) = english.get(&key) {
            if *en != value {
                out.insert(en.clone(), value);
            }
        }
    }
    out
}

/// Where the translations live; set once at start.
pub fn set_dir(config_dir: &Path) {
    *DIR.lock().unwrap_or_else(|e| e.into_inner()) = Some(config_dir.join("translations"));
}

pub fn dir() -> Option<PathBuf> {
    DIR.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// (Re)loads the translations of `locale`.
pub fn load(locale: &str) {
    let map = dir().map(|d| build_map(&d, locale)).unwrap_or_default();
    *MAP.write().unwrap_or_else(|e| e.into_inner()) = Some(map);
}

/// The translated template for an English one.
pub fn lookup(template: &str) -> Option<String> {
    MAP.read().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(template).cloned())
}

/// `getInstalledLocales`: the locale tags of the files in both directories.
pub fn installed() -> Vec<String> {
    let Some(base) = dir() else { return Vec::new() };
    let mut tags: Vec<String> = Vec::new();
    for d in [base.clone(), base.join("repo")] {
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some(stem) = name.strip_suffix(".properties") {
                    let tag = stem.strip_prefix("messages_").unwrap_or(stem).replace('_', "-");
                    if !tags.contains(&tag) {
                        tags.push(tag);
                    }
                }
            }
        }
    }
    tags.sort();
    tags
}

/// `getTranslationsMetadata`: the languages with progress above zero.
pub fn parse_languages(body: &[u8]) -> Result<Vec<LanguageInfo>, String> {
    let json: serde_json::Value = serde_json::from_slice(body).map_err(|e| e.to_string())?;
    let langs = json.get("languages").and_then(|v| v.as_object()).ok_or("no languages")?;
    let mut out = Vec::new();
    for (id, data) in langs {
        let progress = data.get("progress").and_then(|v| v.as_i64()).unwrap_or(0);
        if progress <= 0 {
            continue;
        }
        out.push(LanguageInfo {
            id: id.clone(),
            name: data.get("name").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
            locale: data.get("localeTag").and_then(|v| v.as_str()).unwrap_or(id).to_owned(),
            progress,
            contributors: data
                .get("contributors")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|c| c.get("name").and_then(|n| n.as_str()).map(str::to_owned)).collect())
                .unwrap_or_default(),
        });
    }
    if out.len() >= MAX_LANGUAGES {
        return Err("More than 100 languages - cancelling download".into());
    }
    Ok(out)
}

/// `getAvailableLanguages`.
pub fn available(client: &dyn Bytebin) -> Result<Vec<LanguageInfo>, String> {
    let body = client.get(INFO_PATH).map_err(|e| format!("{e:?}"))?;
    parse_languages(&body)
}

/// `downloadAndInstallTranslations`: replaces the repository files; `report` hears each language and each failure.
pub fn install(client: &dyn Bytebin, languages: &[LanguageInfo], mut report: impl FnMut(&LanguageInfo, bool)) {
    let Some(base) = dir() else { return };
    let repo = base.join("repo");
    let _ = std::fs::create_dir_all(&repo);
    if let Ok(rd) = std::fs::read_dir(&repo) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().ends_with(".properties") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    for lang in languages {
        report(lang, true);
        let file = repo.join(format!("{}.properties", lang.locale.replace('-', "_")));
        let ok = match client.get(&format!("{DOWNLOAD_PATH}{}", lang.id)) {
            Ok(bytes) => std::fs::write(&file, bytes).is_ok(),
            Err(HttpError::Status(..) | HttpError::Other(_)) => false,
        };
        if !ok {
            report(lang, false);
        }
    }
}

/// The metadata service (the test double replaces it in tests).
pub fn with_service<R>(f: impl FnOnce(&dyn Bytebin) -> R) -> R {
    crate::horizon_luckperms_core::bytebin::with_client(METADATA, f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties_unescape_and_apostrophes() {
        let m = parse_properties("# c\na=Don''t \\u00e4\\n\nb : x\n");
        assert_eq!(m.get("a").map(String::as_str), Some("Don't \u{e4}\n"));
        assert_eq!(m.get("b").map(String::as_str), Some("x"));
    }

    #[test]
    fn english_is_not_translated_and_a_file_maps_by_english_value() {
        let dir = std::env::temp_dir().join(format!("lp_tr_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("de.properties"), "luckperms.command.translations.installing=Installiere\n").unwrap();
        assert!(build_map(&dir, "en").is_empty());
        let m = build_map(&dir, "de_DE");
        assert_eq!(m.get("Installing translations, please wait...").map(String::as_str), Some("Installiere"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn languages_with_no_progress_are_dropped() {
        let json = br#"{"cacheMaxAge":1,"languages":{"a":{"name":"German","localeTag":"de-DE","progress":50,"contributors":[{"name":"x"}]},"b":{"name":"Z","localeTag":"zz","progress":0,"contributors":[]}}}"#;
        let l = parse_languages(json).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].locale, "de-DE");
        assert_eq!(l[0].contributors, vec!["x".to_string()]);
    }
}
