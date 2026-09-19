//! Native-messaging host registration for Chrome and Edge, current user only.
//! Written only when the user clicks 「连接」; 「断开」 removes it.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const HOST_NAME: &str = "com.stacker.webchat";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Browser {
    Chrome,
    Edge,
}

impl Browser {
    pub const ALL: [Browser; 2] = [Browser::Chrome, Browser::Edge];

    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "chrome" => Ok(Self::Chrome),
            "edge" => Ok(Self::Edge),
            _ => Err("E_REQUEST".into()),
        }
    }

    pub fn subkey(self) -> String {
        let vendor = match self {
            Self::Chrome => "Google\\Chrome",
            Self::Edge => "Microsoft\\Edge",
        };
        format!("Software\\{vendor}\\NativeMessagingHosts\\{HOST_NAME}")
    }
}

/// HKCU access; tests use a fake.
pub trait Registry {
    fn read_default(&self, subkey: &str) -> Option<String>;
    fn write_default(&self, subkey: &str, value: &str) -> Result<(), String>;
    fn delete_key(&self, subkey: &str) -> Result<(), String>;
}

pub struct UserRegistry;

#[cfg(windows)]
impl Registry for UserRegistry {
    fn read_default(&self, subkey: &str) -> Option<String> {
        use winreg::enums::HKEY_CURRENT_USER;
        winreg::RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(subkey)
            .ok()?
            .get_value::<String, _>("")
            .ok()
    }

    fn write_default(&self, subkey: &str, value: &str) -> Result<(), String> {
        use winreg::enums::HKEY_CURRENT_USER;
        let (key, _) = winreg::RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(subkey)
            .map_err(|_| "E_REGISTRY".to_string())?;
        key.set_value("", &value.to_string())
            .map_err(|_| "E_REGISTRY".to_string())
    }

    fn delete_key(&self, subkey: &str) -> Result<(), String> {
        use winreg::enums::HKEY_CURRENT_USER;
        match winreg::RegKey::predef(HKEY_CURRENT_USER).delete_subkey(subkey) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("E_REGISTRY".into()),
        }
    }
}

#[cfg(not(windows))]
impl Registry for UserRegistry {
    fn read_default(&self, _subkey: &str) -> Option<String> {
        None
    }
    fn write_default(&self, _subkey: &str, _value: &str) -> Result<(), String> {
        Err("E_REGISTRY".into())
    }
    fn delete_key(&self, _subkey: &str) -> Result<(), String> {
        Ok(())
    }
}

pub fn manifest_path(root: &Path) -> PathBuf {
    root.join("native-messaging")
        .join(format!("{HOST_NAME}.json"))
}

pub fn manifest_json(exe: &Path, extension_id: &str) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "name": HOST_NAME,
        "description": "Stacker web chats bridge",
        "path": exe.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{extension_id}/")],
    }))
    .unwrap_or_default()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostState {
    Off,
    Connected,
    /// Registered, but for another manifest or another stacker.exe.
    Stale,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserStatus {
    pub browser: Browser,
    pub state: HostState,
    /// The manifest path the browser's key points at, empty when not registered.
    pub registered: String,
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

fn manifest_exe(root: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(manifest_path(root)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value["path"].as_str().map(PathBuf::from)
}

/// Connected only when the key points at our manifest and the manifest points at this exe.
pub fn status(reg: &dyn Registry, root: &Path, exe: &Path) -> Vec<BrowserStatus> {
    let ours = manifest_path(root);
    let listed_exe = manifest_exe(root);
    Browser::ALL
        .iter()
        .map(|&browser| {
            let registered = reg.read_default(&browser.subkey()).unwrap_or_default();
            let state = if registered.is_empty() {
                HostState::Off
            } else if same_path(Path::new(&registered), &ours)
                && listed_exe.as_deref().is_some_and(|m| same_path(m, exe))
            {
                HostState::Connected
            } else {
                HostState::Stale
            };
            BrowserStatus {
                browser,
                state,
                registered,
            }
        })
        .collect()
}

pub fn connect(
    reg: &dyn Registry,
    browser: Browser,
    root: &Path,
    exe: &Path,
    extension_id: &str,
) -> Result<(), String> {
    let path = manifest_path(root);
    let dir = path.parent().ok_or("E_STORAGE")?;
    std::fs::create_dir_all(dir).map_err(|_| "E_STORAGE".to_string())?;
    std::fs::write(&path, manifest_json(exe, extension_id)).map_err(|_| "E_STORAGE".to_string())?;
    reg.write_default(&browser.subkey(), &path.to_string_lossy())
}

/// Removes the browser's key; the manifest goes too once no browser points at it.
pub fn disconnect(reg: &dyn Registry, browser: Browser, root: &Path) -> Result<(), String> {
    reg.delete_key(&browser.subkey())?;
    let ours = manifest_path(root);
    let still_used = Browser::ALL.iter().any(|b| {
        reg.read_default(&b.subkey())
            .is_some_and(|p| same_path(Path::new(&p), &ours))
    });
    if !still_used {
        match std::fs::remove_file(&ours) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("E_STORAGE".into()),
        }
    }
    Ok(())
}

pub fn pick_extension_dir(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|d| d.join("manifest.json").is_file())
        .cloned()
}

/// Installed and portable Stacker ship the unpacked extension next to stacker.exe;
/// development builds also look at `<repo>/extension/dist`.
fn extension_candidates() -> Vec<PathBuf> {
    let mut list = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        list.push(dir.join("extension"));
    }
    if cfg!(debug_assertions) {
        if let Some(repo) = Path::new(env!("CARGO_MANIFEST_DIR")).parent() {
            list.push(repo.join("extension").join("dist"));
        }
    }
    list
}

/// The extension folder and whether it exists; when missing, where it should be.
pub fn extension_dir() -> (PathBuf, bool) {
    let candidates = extension_candidates();
    match pick_extension_dir(&candidates) {
        Some(dir) => (dir, true),
        None => (candidates.last().cloned().unwrap_or_default(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeRegistry(RefCell<HashMap<String, String>>);

    impl Registry for FakeRegistry {
        fn read_default(&self, subkey: &str) -> Option<String> {
            self.0.borrow().get(subkey).cloned()
        }
        fn write_default(&self, subkey: &str, value: &str) -> Result<(), String> {
            self.0.borrow_mut().insert(subkey.into(), value.into());
            Ok(())
        }
        fn delete_key(&self, subkey: &str) -> Result<(), String> {
            self.0.borrow_mut().remove(subkey);
            Ok(())
        }
    }

    const ID: &str = "abcdefghijklmnopabcdefghijklmnop";

    fn states(reg: &FakeRegistry, root: &Path, exe: &Path) -> Vec<HostState> {
        status(reg, root, exe)
            .into_iter()
            .map(|s| s.state)
            .collect()
    }

    #[test]
    fn keys_are_the_documented_current_user_locations() {
        assert_eq!(
            Browser::Chrome.subkey(),
            "Software\\Google\\Chrome\\NativeMessagingHosts\\com.stacker.webchat"
        );
        assert_eq!(
            Browser::Edge.subkey(),
            "Software\\Microsoft\\Edge\\NativeMessagingHosts\\com.stacker.webchat"
        );
        assert_eq!(Browser::parse("edge"), Ok(Browser::Edge));
        assert_eq!(Browser::parse("firefox"), Err("E_REQUEST".to_string()));
    }

    #[test]
    fn manifest_names_the_exe_and_only_our_extension() {
        let exe = Path::new("C:\\Program Files\\Stacker\\stacker.exe");
        let v: serde_json::Value = serde_json::from_str(&manifest_json(exe, ID)).unwrap();
        assert_eq!(v["name"], HOST_NAME);
        assert_eq!(v["type"], "stdio");
        assert_eq!(v["path"], "C:\\Program Files\\Stacker\\stacker.exe");
        assert_eq!(
            v["allowed_origins"],
            serde_json::json!([format!("chrome-extension://{ID}/")])
        );
        assert!(manifest_path(Path::new("D:\\data"))
            .ends_with("native-messaging\\com.stacker.webchat.json"));
    }

    #[test]
    fn connect_status_and_disconnect() {
        let dir = tempfile::tempdir().unwrap();
        let reg = FakeRegistry::default();
        let exe = dir.path().join("stacker.exe");
        assert_eq!(
            states(&reg, dir.path(), &exe),
            vec![HostState::Off, HostState::Off]
        );

        connect(&reg, Browser::Chrome, dir.path(), &exe, ID).unwrap();
        let manifest = manifest_path(dir.path());
        assert!(manifest.is_file());
        assert_eq!(
            reg.read_default(&Browser::Chrome.subkey()).unwrap(),
            manifest.to_string_lossy()
        );
        assert_eq!(
            states(&reg, dir.path(), &exe),
            vec![HostState::Connected, HostState::Off]
        );
        // Another Stacker build is running now: the registration points at the old exe.
        let other = dir.path().join("other.exe");
        assert_eq!(states(&reg, dir.path(), &other)[0], HostState::Stale);

        connect(&reg, Browser::Edge, dir.path(), &exe, ID).unwrap();
        disconnect(&reg, Browser::Chrome, dir.path()).unwrap();
        assert!(manifest.is_file(), "Edge still uses the manifest");
        assert_eq!(
            states(&reg, dir.path(), &exe),
            vec![HostState::Off, HostState::Connected]
        );
        disconnect(&reg, Browser::Edge, dir.path()).unwrap();
        assert!(!manifest.exists());
        disconnect(&reg, Browser::Edge, dir.path()).unwrap();
    }

    #[test]
    fn a_key_pointing_elsewhere_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let reg = FakeRegistry::default();
        reg.write_default(
            &Browser::Chrome.subkey(),
            "D:\\old\\com.stacker.webchat.json",
        )
        .unwrap();
        let exe = dir.path().join("stacker.exe");
        assert_eq!(states(&reg, dir.path(), &exe)[0], HostState::Stale);
    }

    #[test]
    fn the_first_folder_with_a_manifest_is_the_extension() {
        let dir = tempfile::tempdir().unwrap();
        let installed = dir.path().join("installed");
        let dev = dir.path().join("dev");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::create_dir_all(&dev).unwrap();
        std::fs::write(dev.join("manifest.json"), "{}").unwrap();
        assert_eq!(
            pick_extension_dir(&[installed.clone(), dev.clone()]),
            Some(dev)
        );
        assert_eq!(pick_extension_dir(&[installed]), None);
    }

    /// Controller-only (Task 19): registers the debug build for Chrome and keeps it.
    /// `cargo test --manifest-path src-tauri/Cargo.toml --lib live_register_chrome -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_register_chrome() {
        let exe = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("debug")
            .join("stacker.exe");
        assert!(exe.is_file(), "build the debug exe first: cargo build");
        assert!(
            std::env::var_os("STACKER_WEBCHAT_DIR").is_none(),
            "unset STACKER_WEBCHAT_DIR"
        );
        let root = crate::webchat::root();
        connect(
            &UserRegistry,
            Browser::Chrome,
            &root,
            &exe,
            crate::webchat::extension_id(),
        )
        .unwrap();
        let now = status(&UserRegistry, &root, &exe);
        println!("{now:?}");
        assert_eq!(now[0].state, HostState::Connected);
    }
}
