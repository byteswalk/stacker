//! The folders Stacker keeps its own things in, listed in Settings → About so the user can see
//! where the program, its data and everything it downloaded and installed live.
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
pub struct StoragePlace {
    pub key: &'static str,
    pub path: String,
    pub exists: bool,
    /// What the folder holds on disk; links are not followed.
    pub bytes: u64,
}

fn places() -> Vec<(&'static str, PathBuf)> {
    let program = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .unwrap_or_default();
    let data = dirs::config_dir().unwrap_or_default().join("stacker");
    vec![
        ("program", program),
        ("data", data),
        ("runtimes", crate::installer::managed_runtimes_root()),
        ("tools", crate::installer::managed_tools_root()),
        ("conversations", crate::sessions::annotations::root()),
        ("logs", crate::settings::logs_dir()),
    ]
}

fn place_path(key: &str) -> Option<PathBuf> {
    places()
        .into_iter()
        .find(|(name, _)| *name == key)
        .map(|(_, path)| path)
}

#[tauri::command]
pub async fn storage_places() -> Result<Vec<StoragePlace>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        // Each folder is measured on its own thread: the slowest one sets the wait, not the sum.
        let places = places();
        std::thread::scope(|scope| {
            let sizes: Vec<_> = places
                .iter()
                .map(|(_, path)| scope.spawn(|| crate::sessions::claude_catalog::tree_size(path)))
                .collect();
            places
                .iter()
                .zip(sizes)
                .map(|((key, path), size)| StoragePlace {
                    key,
                    exists: path.is_dir(),
                    bytes: size.join().unwrap_or(0),
                    path: path.to_string_lossy().into_owned(),
                })
                .collect()
        })
    })
    .await
    .map_err(|error| error.to_string())
}

/// Opens one of the listed folders by its key; any other path is refused.
#[tauri::command]
pub fn storage_place_open(key: String) -> Result<(), String> {
    let path = place_path(&key).ok_or_else(|| format!("未知的存储位置：{key}"))?;
    if !path.is_dir() {
        return Err("这个文件夹还没有创建".into());
    }
    std::process::Command::new("explorer.exe")
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("打开文件夹失败：{error}"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_place_has_a_distinct_key_and_unknown_keys_are_refused() {
        let places = super::places();
        let mut keys: Vec<_> = places.iter().map(|(key, _)| *key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), places.len());
        assert!(super::place_path("runtimes").is_some());
        assert!(super::place_path(r"C:\Windows").is_none());
        assert!(super::storage_place_open(r"C:\Windows".into()).is_err());
    }
}
