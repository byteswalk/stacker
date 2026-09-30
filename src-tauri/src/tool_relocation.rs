//! Tools a portable build installed beside itself, moved to where they survive an upgrade.
//!
//! A portable build used to put the runtimes and tools it installed in `data\` next to its own
//! executable. Every new version is a new folder, so each upgrade left those installs behind —
//! and removing an old version's folder removed them outright, with environment variables
//! still pointing at them. Installs now always go to the per-user data folder. This module finds
//! what is still beside the executable, moves it, rewrites the variables that point there, and
//! drops PATH entries that point into a portable data folder that no longer exists.
//!
//! Nothing moves on its own: the page shows the plan and the user starts it. A variable at
//! system level can only be rewritten with administrator rights, so that part asks once.

use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::winenv::Hive;

/// One folder to move.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Move {
    pub from: String,
    pub to: String,
    /// Something is already at the destination; this one is left where it is.
    pub conflict: bool,
}

/// One variable whose value changes, at user or system level.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvChange {
    pub system: bool,
    pub name: String,
    pub before: String,
    /// Empty when the variable is removed.
    pub after: String,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub moves: Vec<Move>,
    pub env: Vec<EnvChange>,
    /// Whether applying it needs one administrator prompt.
    pub needs_admin: bool,
}

fn norm(path: &str) -> String {
    path.trim()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

/// A path that lives in some portable build's own data folder: the only place this cleans up.
pub fn is_portable_data_path(path: &str) -> bool {
    portable_data_root(path).is_some()
}

/// The `...\data` folder of the portable build a path lives in, spelled as the path spells it.
pub fn portable_data_root(path: &str) -> Option<String> {
    let path = path.trim().replace('/', "\\");
    let lower = path.to_ascii_lowercase();
    if !(lower.contains("stacker") || lower.contains("portable")) {
        return None;
    }
    let at = ["\\data\\tools\\", "\\data\\runtimes\\"]
        .iter()
        .filter_map(|marker| lower.find(marker))
        .min()?;
    Some(path[..at + "\\data".len()].to_string())
}

/// Replaces `old` at the start of any `;`-separated piece of `value`, keeping the rest.
pub fn rewrite(value: &str, old: &str, new: &str) -> String {
    let old_norm = norm(old);
    value
        .split(';')
        .map(|piece| {
            let piece_norm = piece.replace('/', "\\").to_ascii_lowercase();
            if piece_norm.starts_with(&old_norm)
                && (piece_norm.len() == old_norm.len()
                    || piece_norm.as_bytes()[old_norm.len()] == b'\\')
            {
                format!("{new}{}", &piece[old.trim_end_matches(['\\', '/']).len()..])
            } else {
                piece.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Drops the pieces of a PATH-like value that point into a portable data folder that is gone.
pub fn drop_dead(value: &str, exists: &dyn Fn(&str) -> bool) -> String {
    value
        .split(';')
        .filter(|piece| {
            piece.trim().is_empty() || !(is_portable_data_path(piece) && !exists(piece))
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// The portable data folders the variables point into that still exist: what gets moved.
pub fn live_roots(vars: &[(bool, String, String)], exists: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut roots: Vec<String> = Vec::new();
    for (_, _, value) in vars {
        for piece in value.split(';') {
            if let Some(root) = portable_data_root(piece) {
                if exists(&root) && !roots.iter().any(|known| norm(known) == norm(&root)) {
                    roots.push(root);
                }
            }
        }
    }
    roots
}

/// The pieces of the plan that do not touch the disk, so they can be tested.
pub fn plan_env(
    vars: &[(bool, String, String)],
    old_roots: &[String],
    new_root: &str,
    exists: &dyn Fn(&str) -> bool,
) -> Vec<EnvChange> {
    let mut changes = Vec::new();
    for (system, name, before) in vars {
        let mut after = before.clone();
        for old in old_roots {
            after = rewrite(&after, old, new_root);
        }
        after = drop_dead(&after, exists);
        if &after != before {
            changes.push(EnvChange {
                system: *system,
                name: name.clone(),
                before: before.clone(),
                after,
            });
        }
    }
    changes
}

fn env_vars() -> Vec<(bool, String, String)> {
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        use winreg::RegKey;
        let keys = [
            (
                false,
                RegKey::predef(HKEY_CURRENT_USER).open_subkey("Environment"),
            ),
            (
                true,
                RegKey::predef(HKEY_LOCAL_MACHINE)
                    .open_subkey(r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment"),
            ),
        ];
        for (system, key) in keys {
            let Ok(key) = key else { continue };
            for (name, _) in key.enum_values().flatten() {
                if let Ok(value) = key.get_value::<String, _>(&name) {
                    out.push((system, name, value));
                }
            }
        }
    }
    out
}

/// Where this build, if it is a portable one, used to keep what it installed.
fn own_portable_data() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    exe_dir
        .join("portable.flag")
        .is_file()
        .then(|| exe_dir.join("data"))
}

/// Something is at the path: a file, or a folder with anything in it.
fn occupied(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_some(),
        Err(_) => path.exists(),
    }
}

fn children(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default()
}

pub fn plan() -> Plan {
    let new_root = crate::installer::managed_storage_root();
    let vars = env_vars();
    let exists = |path: &str| Path::new(path.trim()).exists();
    // This build's own data folder, and any other version's that a variable still points into.
    let mut old_roots = live_roots(&vars, &exists);
    if let Some(own) = own_portable_data().filter(|dir| dir.is_dir()) {
        let own = own.to_string_lossy().into_owned();
        if !old_roots.iter().any(|known| norm(known) == norm(&own)) {
            old_roots.push(own);
        }
    }
    let mut moves = Vec::new();
    for old in old_roots.iter().map(PathBuf::from) {
        // A tool is one folder; a runtime is a folder per product holding one per version, and
        // each version moves on its own so one already at the destination keeps the others.
        let units = children(&old.join("tools")).into_iter().chain(
            children(&old.join("runtimes"))
                .into_iter()
                .flat_map(|product| children(&product)),
        );
        for from in units.filter(|path| path.is_dir()) {
            let Ok(relative) = from.strip_prefix(&old) else {
                continue;
            };
            let to = new_root.join(relative);
            moves.push(Move {
                from: from.to_string_lossy().into_owned(),
                conflict: occupied(&to),
                to: to.to_string_lossy().into_owned(),
            });
        }
    }
    let env = plan_env(&vars, &old_roots, &new_root.to_string_lossy(), &exists);
    Plan {
        needs_admin: env.iter().any(|change| change.system),
        moves,
        env,
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

/// Moves a folder, across drives if it has to: copy first, delete the original only after.
fn move_dir(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // An empty folder left by an install that never finished is in nobody's way.
    if to.is_dir() && !occupied(to) {
        let _ = std::fs::remove_dir(to);
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy_tree(from, to).map_err(|e| format!("{}：{e}", from.display()))?;
    std::fs::remove_dir_all(from).map_err(|e| format!("{}：{e}", from.display()))
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub moved: usize,
    pub env_changed: usize,
    pub failures: Vec<String>,
}

pub fn apply() -> Outcome {
    let plan = plan();
    let mut outcome = Outcome::default();
    for item in plan.moves.iter().filter(|item| !item.conflict) {
        match move_dir(Path::new(&item.from), Path::new(&item.to)) {
            Ok(()) => outcome.moved += 1,
            Err(error) => outcome.failures.push(error),
        }
    }
    // Variables follow only once everything moved; otherwise they would point at nothing.
    if !outcome.failures.is_empty() {
        return outcome;
    }
    // User-level variables are written directly; system-level ones together, in one prompt.
    let mut system = Vec::new();
    for change in &plan.env {
        if change.system {
            system.push((change.name.clone(), change.after.clone()));
            continue;
        }
        let result = if change.after.is_empty() {
            crate::winenv::remove_in(Hive::User, &change.name)
        } else {
            crate::winenv::set_in(Hive::User, &change.name, &change.after)
        };
        match result {
            Ok(()) => outcome.env_changed += 1,
            Err(error) => outcome.failures.push(format!("{}：{error}", change.name)),
        }
    }
    if !system.is_empty() {
        let count = system.len();
        match crate::winadmin::set_env_system("tool-relocation", system) {
            Ok(()) => outcome.env_changed += count,
            Err(error) => outcome.failures.push(error),
        }
    }
    crate::winenv::broadcast_change();
    outcome
}

#[tauri::command]
pub async fn tool_relocation_plan() -> Plan {
    tauri::async_runtime::spawn_blocking(plan)
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn tool_relocation_apply() -> Result<Outcome, String> {
    tauri::async_runtime::spawn_blocking(apply)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = r"D:\rel\v0.3.4-r39\Stacker-0.3.4-portable-windows-x64\data";
    const NEW: &str = r"C:\Users\me\AppData\Local\Stacker";

    #[test]
    fn a_variable_pointing_into_the_old_folder_follows_it() {
        let before = format!(r"{OLD}\runtimes\gradle\gradle-9.8.0");
        assert_eq!(
            rewrite(&before, OLD, NEW),
            format!(r"{NEW}\runtimes\gradle\gradle-9.8.0")
        );
        // Other pieces of a PATH are untouched, and a lookalike prefix is not a match.
        let path = format!(r"C:\Windows;{OLD}\tools\fnm;{OLD}x\tools\y");
        assert_eq!(
            rewrite(&path, OLD, NEW),
            format!(r"C:\Windows;{NEW}\tools\fnm;{OLD}x\tools\y")
        );
    }

    #[test]
    fn a_dead_portable_entry_is_dropped_and_nothing_else_is() {
        let dead = r"D:\rel\v0.3.4-r10\Stacker-0.3.4-portable-windows-x64\data\tools\fnm";
        let path = format!(r"C:\Windows;{dead};C:\missing\bin");
        let exists = |p: &str| p.eq_ignore_ascii_case(r"C:\Windows");
        // The dead portable entry goes; an ordinary missing folder is none of Stacker's business.
        assert_eq!(drop_dead(&path, &exists), r"C:\Windows;C:\missing\bin");
    }

    #[test]
    fn the_plan_covers_rewrites_and_removals_at_both_levels() {
        let vars = vec![
            (
                true,
                "GRADLE_HOME".to_string(),
                format!(r"{OLD}\runtimes\gradle\gradle-9.8.0"),
            ),
            (
                false,
                "Path".to_string(),
                r"C:\Windows;D:\rel\v0.3.4-r10\Stacker-0.3.4-portable-windows-x64\data\tools\fnm"
                    .to_string(),
            ),
            (
                false,
                "JAVA_HOME".to_string(),
                r"D:\JavaSDKs\jdk-25".to_string(),
            ),
        ];
        let exists = |p: &str| p.eq_ignore_ascii_case(r"C:\Windows");
        let changes = plan_env(&vars, &[OLD.to_string()], NEW, &exists);
        assert_eq!(changes.len(), 2, "{changes:?}");
        assert!(changes
            .iter()
            .any(|c| c.system && c.name == "GRADLE_HOME" && c.after.starts_with(NEW)));
        assert!(changes
            .iter()
            .any(|c| !c.system && c.name == "Path" && c.after == r"C:\Windows"));
    }

    #[test]
    fn a_variable_that_only_pointed_at_a_dead_folder_is_removed() {
        let vars = vec![(
            false,
            "GRADLE_HOME".to_string(),
            r"D:\rel\v0.3.4-r12\Stacker-0.3.4-portable-windows-x64\data\runtimes\gradle\gradle-9.7.0".to_string(),
        )];
        let changes = plan_env(&vars, &[], NEW, &|_| false);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].after, "");
    }

    #[test]
    fn other_versions_folders_are_found_through_the_variables() {
        let vars = vec![
            (
                true,
                "GRADLE_HOME".to_string(),
                format!(r"{OLD}\runtimes\gradle\gradle-9.8.0"),
            ),
            (
                false,
                "Path".to_string(),
                format!(r"{OLD}\runtimes\gradle\gradle-9.8.0\bin;C:\Windows"),
            ),
            (
                false,
                "Path".to_string(),
                r"D:\gone\Stacker-0.3.4-portable-windows-x64\data\tools\fnm".to_string(),
            ),
        ];
        let exists = |p: &str| p.eq_ignore_ascii_case(OLD);
        assert_eq!(live_roots(&vars, &exists), vec![OLD.to_string()]);
    }

    #[test]
    fn an_empty_leftover_folder_does_not_block_a_move() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("old").join("gradle-9.8.0");
        let to = dir.path().join("new").join("gradle-9.8.0");
        std::fs::create_dir_all(from.join("bin")).unwrap();
        std::fs::write(from.join("bin").join("gradle.bat"), b"x").unwrap();
        std::fs::create_dir_all(&to).unwrap();
        assert!(!occupied(&to));
        move_dir(&from, &to).unwrap();
        assert!(to.join("bin").join("gradle.bat").is_file());
        assert!(!from.exists());
        assert!(occupied(&to));
    }

    #[test]
    #[ignore = "reads this machine's environment"]
    fn the_plan_on_this_machine() {
        println!("{:#?}", plan());
    }

    #[test]
    fn only_a_portable_data_folder_counts() {
        assert!(is_portable_data_path(
            r"D:\x\Stacker-0.3.4-portable-windows-x64\data\tools\fnm"
        ));
        assert!(!is_portable_data_path(r"D:\data\tools\something"));
        assert!(!is_portable_data_path(r"C:\Program Files\Git\cmd"));
    }
}
