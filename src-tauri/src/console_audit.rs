//! A guard against the bug class this module is named for: a console program started from a
//! GUI app gets its own window unless Windows is told otherwise, and one that flashes up and
//! disappears while the user is doing something else is alarming and impossible to read.
//!
//! The rule is simple enough to check mechanically: every `Command::new` either hides its
//! window, or is one of the few places that means to show one.

#![cfg(test)]

use std::path::Path;

/// Spawns that are meant to be seen: a terminal the user watches an install in, and the
/// file manager, which has no console to begin with.
const VISIBLE_ON_PURPOSE: &[&str] = &[
    // Windows Terminal / cmd `start`: the install runs where the user can read it.
    "installer.rs",
    // explorer.exe and the platform's opener show a window because that is the point.
    "settings.rs",
    "update.rs",
    "agents/mod.rs",
    "space_analysis/mod.rs",
    "sessions/commands.rs",
];

/// How a spawn says it is hidden, directly or through a helper that hides it.
const HIDES: &[&str] = &[
    "creation_flags",
    "hidden(",
    "hide(",
    "hide_window(",
    "hide_console(",
    "command_for_path",
    "command_output_timeout_named",
    "run_program",
];

fn sources(dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn every_command_hides_its_console_or_means_to_show_one() {
    let mut files = Vec::new();
    sources(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src").as_path(),
        &mut files,
    );
    assert!(files.len() > 20, "no sources found to audit");

    let mut offenders = Vec::new();
    for file in files {
        let relative = file
            .rsplit_once("/src/")
            .map(|(_, rest)| rest)
            .unwrap_or(&file);
        if relative == "console_audit.rs" || VISIBLE_ON_PURPOSE.contains(&relative) {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        // Test code spawns helpers of its own; only what ships is audited.
        let tests = lines
            .iter()
            .position(|line| line.trim_start().starts_with("mod tests"))
            .unwrap_or(lines.len());
        for (index, line) in lines.iter().enumerate().take(tests) {
            if !line.contains("Command::new(") {
                continue;
            }
            let start = index.saturating_sub(8);
            let window = lines[start..(index + 16).min(lines.len())].join("\n");
            if !HIDES.iter().any(|needle| window.contains(needle)) {
                offenders.push(format!("{relative}:{}  {}", index + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these commands would flash a console window:\n{}",
        offenders.join("\n")
    );
}
