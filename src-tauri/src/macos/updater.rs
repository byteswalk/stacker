//! Installing an update on macOS. The downloaded zip (already checked against its SHA-256
//! and the release signature) is unpacked beside the running app and checked again by
//! `codesign`; a small shell script waits for Stacker to quit, swaps the two bundles, and
//! opens the new one. The new Stacker removes the old bundle when it starts; if it has not
//! done so after a while, the script puts the old one back.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const OLD_PREFIX: &str = ".Stacker-old-";
const STAGE_PREFIX: &str = ".Stacker-update-";
/// How long the new version gets to start before the old one is put back.
const START_GRACE_SECS: u32 = 25;

pub struct Staged {
    /// The bundle being replaced, e.g. `/Applications/Stacker.app`.
    pub app: PathBuf,
    /// The verified new bundle beside it.
    pub new_app: PathBuf,
    pub stage_dir: PathBuf,
    pub old_version: String,
}

/// The `.app` this process runs from.
pub fn running_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    exe.ancestors()
        .find(|dir| dir.extension().is_some_and(|ext| ext == "app"))
        .map(Path::to_path_buf)
}

fn plist(app: &Path, key: &str) -> Option<String> {
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(app.join("Contents").join("Info.plist"))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !text.is_empty()).then_some(text)
}

/// What `codesign` says any copy of this app must satisfy.
fn designated_requirement(app: &Path) -> Option<String> {
    let output = Command::new("/usr/bin/codesign")
        .args(["-d", "-r-"])
        .arg(app)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    text.lines()
        .find_map(|line| line.strip_prefix("designated => "))
        .map(str::to_string)
}

fn run(program: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("无法运行 {program}：{e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Unpacks and checks the update next to the running app.
pub fn stage(zip: &Path, version: &str) -> Result<Staged, String> {
    let manual = "请到发布页下载新版 .dmg 手动安装";
    let app = running_bundle().ok_or(format!(
        "当前不是从 Stacker.app 运行，无法自动更新，{manual}"
    ))?;
    stage_beside(&app, zip, version)
}

fn stage_beside(app: &Path, zip: &Path, version: &str) -> Result<Staged, String> {
    let manual = "请到发布页下载新版 .dmg 手动安装";
    let app = app.to_path_buf();
    let parent = app.parent().ok_or("找不到 Stacker 所在的文件夹")?;
    let stage_dir = parent.join(format!("{STAGE_PREFIX}{version}"));
    let _ = std::fs::remove_dir_all(&stage_dir);
    std::fs::create_dir(&stage_dir)
        .map_err(|e| format!("无法在 {} 里准备更新（{e}），{manual}", parent.display()))?;
    let fail = |message: String| {
        let _ = std::fs::remove_dir_all(&stage_dir);
        let _ = std::fs::remove_file(zip);
        message
    };
    run(
        "/usr/bin/ditto",
        &[
            "-x".as_ref(),
            "-k".as_ref(),
            zip.as_os_str(),
            stage_dir.as_os_str(),
        ],
    )
    .map_err(|e| fail(format!("解压更新包失败：{e}")))?;
    let _ = std::fs::remove_file(zip);
    let new_app = stage_dir.join(app.file_name().unwrap_or("Stacker.app".as_ref()));
    if !new_app.is_dir() {
        return Err(fail("更新包里没有 Stacker.app".into()));
    }
    run(
        "/usr/bin/codesign",
        &[
            "--verify".as_ref(),
            "--deep".as_ref(),
            "--strict".as_ref(),
            new_app.as_os_str(),
        ],
    )
    .map_err(|e| fail(format!("新版本的代码签名校验不通过：{e}")))?;
    // Signed with Stacker's certificate, the running app names that certificate in its
    // designated requirement; the new one has to name the same. (An ad hoc test build names
    // only its own code, which every build changes.)
    if let Some(required) = designated_requirement(&app).filter(|r| r.contains("certificate leaf"))
    {
        if designated_requirement(&new_app).as_deref() != Some(required.as_str()) {
            return Err(fail("新版本不是用同一张证书签名的".into()));
        }
    }
    let ours = plist(&app, "CFBundleIdentifier");
    if ours.is_none() || plist(&new_app, "CFBundleIdentifier") != ours {
        return Err(fail("更新包不是 Stacker".into()));
    }
    if plist(&new_app, "CFBundleShortVersionString").as_deref() != Some(version) {
        return Err(fail("更新包的版本号和发布信息不一致".into()));
    }
    Ok(Staged {
        old_version: plist(&app, "CFBundleShortVersionString").unwrap_or_default(),
        app,
        new_app,
        stage_dir,
    })
}

/// The script that swaps the bundles once this process is gone, and puts the old one back
/// when the new one never starts.
fn script() -> &'static str {
    r#"pid="$1"; app="$2"; new="$3"; stage="$4"; backup="$5"; from="$6"; grace="$7"
while kill -0 "$pid" 2>/dev/null; do sleep 0.2; done
mv "$app" "$backup" || exit 1
if ! mv "$new" "$app"; then mv "$backup" "$app"; open -n "$app"; exit 1; fi
rmdir "$stage" 2>/dev/null
open -n "$app" --args --updated-from="$from"
i=0
while [ $i -lt "$grace" ]; do
  [ -e "$backup" ] || exit 0
  sleep 1; i=$((i+1))
done
# The new version takes the backup by renaming it; whichever rename comes first wins.
mv "$backup" "$backup.restoring" 2>/dev/null || exit 0
ps -axo pid=,command= | while read -r p cmd; do
  case "$cmd" in "$app/Contents/MacOS/"*) kill "$p" 2>/dev/null ;; esac
done
sleep 1
rm -rf "$app" && mv "$backup.restoring" "$app" && open -n "$app" --args --update-failed="$from"
"#
}

pub fn swap_after_exit(staged: &Staged) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let parent = staged.app.parent().ok_or("找不到 Stacker 所在的文件夹")?;
    let backup = parent.join(format!("{OLD_PREFIX}{}.app", staged.old_version));
    let _ = std::fs::remove_dir_all(&backup);
    Command::new("/bin/sh")
        .arg("-c")
        .arg(script())
        .arg("stacker-update")
        .arg(std::process::id().to_string())
        .arg(&staged.app)
        .arg(&staged.new_app)
        .arg(&staged.stage_dir)
        .arg(&backup)
        .arg(&staged.old_version)
        .arg(START_GRACE_SECS.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Its own process group, so Stacker quitting does not take it along.
        .process_group(0)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("无法启动更新程序：{e}"))
}

/// At startup: the old bundle an update left beside this one goes, which also tells the
/// update script the new version started; so do stage folders an update never finished.
pub fn clean_up_after_update() {
    let Some(app) = running_bundle() else {
        return;
    };
    let Some(parent) = app.parent() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if path == app {
            continue;
        }
        // A backup being restored belongs to the update script.
        if name.ends_with(".restoring") {
            continue;
        }
        if name.starts_with(OLD_PREFIX) || name.starts_with(STAGE_PREFIX) {
            // Taken by a rename first, so the script cannot start restoring it meanwhile.
            let taken = if name.starts_with(OLD_PREFIX) && !name.ends_with(".done") {
                let done = path.with_file_name(format!("{name}.done"));
                match std::fs::rename(&path, &done) {
                    Ok(()) => done,
                    Err(_) => continue,
                }
            } else {
                path.clone()
            };
            match std::fs::remove_dir_all(&taken) {
                Ok(()) => log::info!("removed what an update left: {}", path.display()),
                Err(e) => log::warn!("could not remove {}: {e}", path.display()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole update on real bundles, by hand on a Mac: `STACKER_TEST_APP` names an installed
    /// Stacker.app and `STACKER_TEST_ZIP` an update zip of the same version.
    #[test]
    #[ignore = "needs a built Stacker.app and its update zip"]
    fn a_real_update_zip_is_staged_checked_and_swapped_in() {
        let (Ok(installed), Ok(zip)) = (
            std::env::var("STACKER_TEST_APP"),
            std::env::var("STACKER_TEST_ZIP"),
        ) else {
            panic!("set STACKER_TEST_APP and STACKER_TEST_ZIP");
        };
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Stacker.app");
        run("/usr/bin/ditto", &[installed.as_ref(), app.as_os_str()]).unwrap();
        let download = dir.path().join("update.zip");
        std::fs::copy(&zip, &download).unwrap();
        let version = plist(&app, "CFBundleShortVersionString").unwrap();

        // A wrong version in the release is refused and leaves nothing behind.
        let refused = stage_beside(&app, &download, "9.9.9").err().unwrap();
        assert!(refused.contains("版本号"), "{refused}");
        assert!(!dir.path().join(".Stacker-update-9.9.9").exists());

        std::fs::copy(&zip, &download).unwrap();
        let staged = stage_beside(&app, &download, &version).unwrap();
        assert!(staged
            .new_app
            .join("Contents")
            .join("MacOS")
            .join("stacker")
            .is_file());
        assert!(!download.exists(), "the zip is gone once unpacked");

        // The swap, with `open` recorded instead of run.
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let log = dir.path().join("opened");
        std::fs::write(
            bin.join("open"),
            format!("#!/bin/sh\necho \"$@\" >> '{}'\n", log.display()),
        )
        .unwrap();
        run("/bin/chmod", &["+x".as_ref(), bin.join("open").as_os_str()]).unwrap();
        let backup = dir
            .path()
            .join(format!(".Stacker-old-{}.app", staged.old_version));
        let status = Command::new("/bin/sh")
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .arg("-c")
            .arg(script())
            .arg("t")
            .arg("999999")
            .arg(&staged.app)
            .arg(&staged.new_app)
            .arg(&staged.stage_dir)
            .arg(&backup)
            .arg(&staged.old_version)
            .arg("1")
            .status()
            .unwrap();
        assert!(status.success());
        run(
            "/usr/bin/codesign",
            &[
                "--verify".as_ref(),
                "--deep".as_ref(),
                "--strict".as_ref(),
                app.as_os_str(),
            ],
        )
        .unwrap();
        assert!(std::fs::read_to_string(&log)
            .unwrap()
            .contains("--updated-from="));
    }

    /// The swap, run for real on folders standing in for bundles.
    #[test]
    fn the_script_swaps_the_bundles_and_puts_the_old_one_back_when_the_new_one_never_starts() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("Stacker.app");
        let stage = dir.path().join(".Stacker-update-2.0");
        let new = stage.join("Stacker.app");
        let backup = dir.path().join(".Stacker-old-1.0.app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("v"), "1.0").unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("v"), "2.0").unwrap();
        // `open` would start a real app; a stand-in records the call instead.
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let log = dir.path().join("opened");
        std::fs::write(
            bin.join("open"),
            format!("#!/bin/sh\necho \"$@\" >> '{}'\n", log.display()),
        )
        .unwrap();
        run("/bin/chmod", &["+x".as_ref(), bin.join("open").as_os_str()]).unwrap();
        let path = format!("{}:/usr/bin:/bin", bin.display());
        let status = Command::new("/bin/sh")
            .env("PATH", &path)
            .arg("-c")
            .arg(script())
            .arg("t")
            .arg("999999")
            .arg(&app)
            .arg(&new)
            .arg(&stage)
            .arg(&backup)
            .arg("1.0")
            .arg("2")
            .status()
            .unwrap();
        assert!(status.success());
        // Nobody removed the backup, so the old version is back and opened again.
        assert_eq!(std::fs::read_to_string(app.join("v")).unwrap(), "1.0");
        assert!(!backup.exists() && !stage.exists());
        let opened = std::fs::read_to_string(&log).unwrap();
        assert!(opened.contains("--updated-from=1.0"), "{opened}");
        assert!(opened.contains("--update-failed=1.0"), "{opened}");
    }
}
