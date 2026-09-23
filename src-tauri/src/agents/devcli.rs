//! Debug builds only: drive agent detection and desktop installs from the command line, so
//! they can be checked on a real machine without clicking through the UI (test binaries
//! cannot load the Windows APIs this code links against).
//!
//! `stacker --dev-agents scan`
//! `stacker --dev-agents install-desktop <product-id>`
//! `stacker --dev-agents update-desktop <product-id>`

use super::registry::spec_by_id;

/// `Some(exit code)` when the arguments asked for a dev command.
pub(crate) fn run(args: &[String]) -> Option<i32> {
    let at = args.iter().position(|arg| arg == "--dev-agents")?;
    let rest: Vec<&str> = args[at + 1..].iter().map(String::as_str).collect();
    Some(match rest.as_slice() {
        ["scan"] => scan(),
        ["install-desktop", id] => desktop(id, super::install::install_desktop_tool),
        ["update-desktop", id] => desktop(id, super::install::update_desktop_tool),
        ["winget", args @ ..] => winget(args),
        ["check-installer", id] => check_installer(id),
        ["python-remove", paths @ ..] => {
            let paths: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
            let results =
                tauri::async_runtime::block_on(crate::python_env::python_remove_runtimes(paths));
            println!("{results:#?}");
            0
        }
        ["venv-inspect", path] => {
            println!("{:#?}", crate::python_venv::inspect(std::path::Path::new(path)));
            0
        }
        ["venv-rebuild", dir, python, keep] => {
            let result = tauri::async_runtime::block_on(crate::python_venv::python_venv_rebuild(
                dir.to_string(),
                python.to_string(),
                *keep == "keep",
            ));
            println!("{result:#?}");
            i32::from(result.is_err())
        }
        ["venv-create", project, python] => {
            let result = tauri::async_runtime::block_on(crate::python_venv::python_venv_create(
                project.to_string(),
                python.to_string(),
            ));
            println!("{result:#?}");
            i32::from(result.is_err())
        }
        ["venv-remove", dir] => {
            let result = tauri::async_runtime::block_on(crate::python_venv::python_venv_remove(
                dir.to_string(),
            ));
            println!("{result:#?}");
            i32::from(result.is_err())
        }
        ["python-report"] => {
            println!("{:#?}", crate::python_env::report());
            0
        }
        _ => {
            eprintln!("usage: --dev-agents scan | install-desktop <id> | update-desktop <id>");
            2
        }
    })
}

fn scan() -> i32 {
    let tools = super::scan_vibe_tools(true);
    match serde_json::to_string_pretty(&tools) {
        Ok(json) => {
            println!("{json}");
            0
        }
        Err(err) => {
            eprintln!("{err}");
            1
        }
    }
}

/// Any WinGet command, run the way installs run it (pseudo console, progress lines).
fn winget(args: &[&str]) -> i32 {
    let context = crate::installer::TaskContext {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        log: std::sync::Arc::new(|line: &str| eprintln!("  | {line}")),
    };
    let args = args.iter().map(|a| a.to_string()).collect();
    let run = || {
        super::install::winget::run_winget_owned(args, std::time::Duration::from_secs(1200), &None)
    };
    match crate::installer::with_task_context(context, run) {
        Ok(text) => {
            println!("ok ({} bytes of output)", text.len());
            0
        }
        Err(err) => {
            eprintln!("{err}");
            1
        }
    }
}

/// Resolves, downloads and verifies a product's official desktop installer without running it.
fn check_installer(id: &str) -> i32 {
    use super::install::direct::*;
    let Some(spec) = spec_by_id(id) else {
        eprintln!("no such product: {id}");
        return 2;
    };
    let Some(installer) = super::registry::direct_desktop_installer(spec.vendor, spec.edition)
    else {
        eprintln!("{id}: no direct installer");
        return 1;
    };
    let resolved = match resolve_installer(installer) {
        Ok(resolved) => resolved,
        Err(err) => {
            eprintln!("{id}: resolve failed: {err}");
            return 1;
        }
    };
    println!(
        "{id}: {} args={:?}",
        resolved.url,
        resolved.silent_args.as_deref().unwrap_or(&[])
    );
    let path = match download_desktop_installer(&spec, &resolved, &None) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("{id}: download failed: {err}");
            return 1;
        }
    };
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let signature = verify_desktop_installer_signature(&path);
    let sha = resolved
        .sha512
        .as_deref()
        .map(|expected| sha512_base64(&path).map(|got| got == expected));
    let _ = std::fs::remove_file(&path);
    println!(
        "{id}: {:.1} MB, signature {signature:?}, sha512 match {sha:?}",
        size as f64 / 1048576.0
    );
    i32::from(signature.is_err())
}

fn desktop(
    id: &str,
    action: fn(&super::registry::ToolSpec, &Option<tauri::Window>) -> Result<String, String>,
) -> i32 {
    let Some(spec) = spec_by_id(id) else {
        eprintln!("no such product: {id}");
        return 2;
    };
    // Inside a task context, so every progress line the action logs is printed as it happens.
    let context = crate::installer::TaskContext {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        log: std::sync::Arc::new(|line: &str| eprintln!("  | {line}")),
    };
    match crate::installer::with_task_context(context, || action(&spec, &None)) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(err) => {
            eprintln!("{err}");
            1
        }
    }
}
