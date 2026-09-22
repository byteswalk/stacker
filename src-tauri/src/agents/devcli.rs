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

fn desktop(
    id: &str,
    action: fn(&super::registry::ToolSpec, &Option<tauri::Window>) -> Result<String, String>,
) -> i32 {
    let Some(spec) = spec_by_id(id) else {
        eprintln!("no such product: {id}");
        return 2;
    };
    match action(&spec, &None) {
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
