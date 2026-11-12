//! `cargo run --example check_original -- <config.yml|contexts.json>...`
//!
//! Runs the port's own loader on the original LuckPerms files and prints a redacted summary: the key
//! paths read, the key paths of the file that were not read, the settings the port does not act on (key
//! names only) and why a file could not be used. Values are never printed, and the file's directory is
//! not printed either (only its name). Exit status 1 when a file cannot be read or used.

use horizon_luckperms_core::original::{inspect_json, inspect_yaml, CONTEXTS_JSON, IGNORED};
use std::path::Path;

fn is_ignored(key: &str) -> bool {
    IGNORED.iter().any(|(p, _)| key == *p || key.starts_with(&format!("{p}.")))
}

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: check_original <config.yml|contexts.json>...");
        std::process::exit(2);
    }
    let mut failed = false;
    for path in &paths {
        let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                println!("== {name}: cannot be read ({})", e.kind());
                failed = true;
                continue;
            }
        };
        let report = if name == CONTEXTS_JSON { inspect_json(&text) } else { inspect_yaml(&text) };
        println!(
            "== {name}: {} read, {} not read, {} not supported",
            report.read.len(),
            report.unread.len(),
            report.notes.len()
        );
        for key in &report.read {
            let tag = if is_ignored(key) { "read, ignored" } else { "read" };
            println!("  {tag:<14} {key}");
        }
        for key in &report.unread {
            println!("  NOT READ       {key}");
        }
        for note in &report.notes {
            println!("  not supported  {note}");
        }
        if let Some(e) = &report.error {
            println!("  error: {e}");
            failed = true;
        }
    }
    if failed {
        std::process::exit(1);
    }
}
