# Agent Catalog Rebuild and Update Task Center Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the patched "AI 办公智能体" implementation with a registry-driven agent catalog, health-checked detection, and a background multi-task update center, and reorganize the sidebar into sections.

**Architecture:** `src-tauri/src/vibe.rs` (3,900 lines) is split into an `agents/` module whose `registry.rs` is the single source of truth for products, shared CLIs, vendors, icons, process signatures and data dirs. Install/update/uninstall work runs as tasks in an `AgentTaskManager` that schedules them by declared resources and gives each task its own cancellation token and log through a thread-local task context read by the existing installer helpers. The frontend drops the blocking `BusyProvider` modal for agent actions and uses a task store fed by an `agent-task` event.

**Tech Stack:** Rust 2021 + Tauri 2, React 19 + TypeScript 6, Vitest, `cargo test`, `ureq`, `chrono`, `tauri-plugin-dialog`.

**Spec:** `docs/superpowers/specs/2026-09-18-agent-catalog-and-task-center-design.md`

## Global Constraints

- Windows 10/11 x64 only; code paths must still compile on non-Windows (`#[cfg(windows)]` guards stay as they are).
- Every existing vendor install/update/uninstall path keeps its current behavior unless a task says otherwise.
- Never read proxy settings from the user environment variables `HTTP_PROXY` / `ALL_PROXY`; never write npm config, Git config or user environment variables during agent actions.
- Never delete anything the user did not confirm; repair and uninstall always go through a confirm dialog in the UI.
- Every new user-facing Chinese string (frontend or Rust) needs an English entry in `src/en.generated.ts`; `npm run check:i18n` must pass.
- Sidebar Chinese section titles are exactly 5 characters (智能体管理 / 开发工具链 / 网络与存储); Chinese menu items are exactly 4 characters; product names (Git, Python, …) are kept as-is.
- Task log retention: last 200 lines per task; finished task retention: last 50 tasks. Download concurrency limit: 3.
- Full check before each commit that touches Rust: `cargo fmt --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`, `cargo test --manifest-path src-tauri/Cargo.toml`. Before each commit that touches the frontend: `npm run typecheck`, `npm run lint`, `npm run test`, `npm run check:i18n`.
- Commit messages end with `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`.
- Work on branch `feat/agents-rebuild`, created from `fix/phase0-stabilize`.

## File Map

Backend (`src-tauri/src/agents/`):

| File | Responsibility |
| --- | --- |
| `mod.rs` | Module declarations, shared output types (`VibeTool`, `VibeSurface`, `InstallInfo`), scan cache, `run_tool_action`, `emit_progress` |
| `registry.rs` | Declarative products, shared CLIs, vendors, desktop detection data; derived `ToolSpec` view; invariants tests |
| `detect.rs` | CLI/desktop detection, version and latest-version lookup, install method detection |
| `health.rs` | PE header check, candidate enumeration, per-candidate health probe |
| `process.rs` | Command resolution and process execution helpers (`run_command_streamed`, `command_for_path`, …) |
| `net.rs` | Proxy selection for Stacker's own downloads and child processes |
| `activity.rs` | Running-agent process scan generated from the registry |
| `install/mod.rs` | Surface-level install/update/uninstall/repair dispatch |
| `install/npm.rs` | npm global install/uninstall |
| `install/winget.rs` | WinGet, Scoop, Chocolatey helpers |
| `install/direct.rs` | Official installer download, signature check, silent run |
| `install/vendor.rs` | Vendor-specific flows |
| `tasks/mod.rs` | `AgentTaskManager`, task records, events, retention |
| `tasks/schedule.rs` | Pure resource scheduler |
| `tasks/runner.rs` | Production runner: resources, action execution, post-check |
| `tasks/plan.rs` | One-click update plan |
| `commands.rs` | All `#[tauri::command]` entry points |

Frontend:

| File | Responsibility |
| --- | --- |
| `src/features/agents/catalogStore.ts` | Agent catalog types and cache store (moved out of `Vibe.tsx`) |
| `src/features/agent-tasks/taskStore.ts` | Task types, event reducer, store, invoke wrappers |
| `src/features/agent-tasks/TaskCenter.tsx` | Header task button, task panel, log modal |
| `src/features/agent-tasks/useTaskToasts.ts` | Global completion toasts and catalog refresh |
| `src/features/agents/UpdatePlanModal.tsx` | One-click update plan dialog |
| `src/pages/Agents.tsx` | 安装更新 page (renamed from `Vibe.tsx`) |
| `src/pages/AgentData.tsx` | 会话数据 page (renamed from `AgentSpace.tsx`) |
| `src/navigation.ts` | Sidebar section definitions |

Deleted: `src-tauri/src/vibe.rs`, `src-tauri/src/work_session.rs`, `src/features/agent-workspace/`.

---

### Task 1: Split `vibe.rs` into the `agents` module (no behavior change)

**Files:**
- Create: every file under `src-tauri/src/agents/` listed in the File Map except `health.rs`, `net.rs`, `tasks/*` (those come later)
- Delete: `src-tauri/src/vibe.rs`
- Modify: `src-tauri/src/lib.rs` (module list and command registrations), `src-tauri/src/space_analysis/monitor.rs:2,329`, `src-tauri/src/work_session.rs` (paths `crate::vibe::` → `crate::agents::`)

**Interfaces:**
- Produces: `crate::agents::{VibeTool, VibeSurface, AgentProcess, scan_agent_activity, command_for_path, managed_agent, managed_cli, managed_desktop, desktop_agent_processes, open_desktop_tool}` at the same signatures they have today. Tauri command names stay `vibe_*`.

- [ ] **Step 1: Create the branch**

```bash
git checkout fix/phase0-stabilize
git checkout -b feat/agents-rebuild
```

- [ ] **Step 2: Move functions into files**

Move each item verbatim from `src-tauri/src/vibe.rs` into the file below. Keep doc comments. Change `fn` visibility to `pub(super)` or `pub(crate)` only where another file needs it; keep everything else private.

| Target file | Items moved from `vibe.rs` |
| --- | --- |
| `agents/mod.rs` | constants `VIBE_PROGRESS_EVENT`, `VIBE_SCAN_WORKERS`, `VIBE_SCAN_CACHE_TTL`; `VibeScanSnapshot`, `VIBE_SCAN_CACHE`; structs `VibeSurface`, `VibeTool`, `AgentEnvironmentSnapshot`; `ManagedCli`, `ManagedDesktop`, `managed_agent`, `managed_cli`, `managed_desktop`; `scan_vibe_tools`, `scan_vibe_tools_cached`, `invalidate_vibe_scan_cache`, `cache_vibe_tool`, `scan_vibe_tool`, `vibe_catalog_tool`, `pending_surface`, `vibe_tool_from_spec`, `unavailable_surface`, `run_tool_action`, `open_desktop_tool`, `open_deepseek_harness_workbench`, `open_external_target`, `build_environment_prompt`, `emit_progress` |
| `agents/registry.rs` | `CliSpec`, `DesktopSpec`, `ToolSpec`, `tool_metadata`, `tool_specs`, `spec_by_id`, `DirectDesktopInstaller`, `direct_desktop_installer`, both `openclaw_desktop_installer_url` variants, `desktop_install_unavailable_reason` |
| `agents/detect.rs` | `DesktopFound`, `cli_surface`, `desktop_surface`, `detect_install_method`, `install_method_label`, `is_conda_path`, `is_npm_shim`, `latest_for_cli`, `trae_cli_latest`, `desktop_internal_latest`, `kimi_work_latest`, `claude_desktop_ready_update`, `parse_claude_ready_update_version`, `detect_desktop_app`, `deepseek_harness_launcher`, both `desktop_appx_package`, both `desktop_registry`, `desktop_start_menu_shortcut`, `desktop_exe_candidate`, `desktop_executable_version`, `normalize_desktop_version`, `desktop_name_matches`, `parse_registered_file`, `executable_from_command`, `is_launchable_desktop_exe`, `find_exe_in_dir`, `start_menu_roots`, `find_shortcut_recursive`, `desktop_candidate_paths`, `npm_latest`, `winget_package_installed`, `winget_available_update`, `winget_latest_version_from_show` |
| `agents/process.rs` | `command_dirs`, `resolve_command`, `resolve_command_including_windowsapps`, `command_for_path`, `apply_fresh_path`, `run_program_probe`, `command_output_timeout_named`, `decode_command_bytes`, `output_text`, `output_all_text`, `first_output_line`, `is_meaningful_output_line`, `run_command_text`, `run_command_streamed`, `log_output_excerpt`, `captured_command_output`, `emit_command_progress`, `terminate_command_tree`, `run_powershell`, `run_powershell_streamed`, `ps_single_quoted` |
| `agents/activity.rs` | `AgentProcess`, `AgentActivitySnapshot`, `scan_agent_environment`, `scan_agent_activity`, `desktop_agent_processes`, `activity_family`, `is_probable_desktop_process` |
| `agents/install/mod.rs` | `install_cli_tool`, `update_cli_tool`, `uninstall_cli_tool`, `install_desktop_tool`, `wait_for_desktop_install`, `update_desktop_tool`, `uninstall_desktop_tool`, `cli_installed_after_action`, `desktop_installed_after_action`, `remove_cli_binary`, `run_uninstall_string`, `uninstall_appx_package` |
| `agents/install/npm.rs` | `npm_install_latest`, `npm_uninstall`, `npm_for_program`, `validate_npm_proxy`, `update_with_npm_source` |
| `agents/install/winget.rs` | `winget_args`, `winget_args_with_proxy`, `configured_proxy_url`, `winget_command`, `known_winget_paths`, `run_winget`, `run_winget_owned`, `winget_query_is_read_only`, `scoop_command`, `choco_command`, `run_scoop`, `run_choco` |
| `agents/install/direct.rs` | `install_desktop_from_official_package`, `download_desktop_installer`, `desktop_download_agent`, `verify_desktop_installer_signature`, `run_downloaded_desktop_installer` |
| `agents/install/vendor.rs` | `install_or_update_claude`, `install_opencode`, `install_openclaw`, `install_hermes`, `run_codex_installer`, `install_or_update_kimi_cli`, `install_or_update_antigravity_cli`, `install_or_update_trae_cli`, `uninstall_trae_cli` |
| `agents/commands.rs` | every `#[tauri::command]` fn: `vibe_tools`, `vibe_tools_refresh`, `vibe_catalog`, `vibe_tool`, `vibe_environment_prompt`, `vibe_tool_action`, `vibe_open_desktop`, `vibe_agent_activity`, `vibe_agent_environment` |

Split the `#[cfg(test)] mod tests` at the end of `vibe.rs`: each test goes into a `#[cfg(test)] mod tests { use super::*; ... }` at the bottom of the file that now owns the function it tests (`desktop_name_matches`, `is_launchable_desktop_exe`, `normalize_desktop_version` → `detect.rs`; `direct_desktop_installer`, `spec_by_id` → `registry.rs`; `winget_args_with_proxy`, `winget_query_is_read_only` → `install/winget.rs`; `winget_latest_version_from_show` → `detect.rs`).

`agents/mod.rs` starts with:

```rust
mod activity;
pub mod commands;
mod detect;
mod install;
mod process;
mod registry;

pub(crate) use activity::{scan_agent_activity, AgentProcess};
pub(crate) use process::command_for_path;
```

`agents/install/mod.rs` starts with:

```rust
mod direct;
mod npm;
mod vendor;
mod winget;
```

- [ ] **Step 3: Rewire callers**

In `src-tauri/src/lib.rs`: replace `mod vibe;` with `mod agents;` and each `vibe::vibe_x` registration with `agents::commands::vibe_x`. In `space_analysis/monitor.rs` replace `crate::vibe::` with `crate::agents::`. In `work_session.rs` replace `crate::vibe::` with `crate::agents::` (this file is deleted in Task 11; it only needs to compile until then).

- [ ] **Step 4: Build and run the full Rust check**

Run: `cargo fmt --manifest-path src-tauri/Cargo.toml; cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings; cargo test --manifest-path src-tauri/Cargo.toml`
Expected: clippy clean, all tests pass with the same count as before the move (178 passed, 2 ignored).

- [ ] **Step 5: Confirm the move is behavior-neutral**

Run: `git diff --stat fix/phase0-stabilize -- src-tauri/src | tail -1`
Expected: insertions and deletions differ only by module headers, `use` lines and visibility keywords (roughly ±300 lines). If a function body changed, revert that change.

- [ ] **Step 6: Commit**

```bash
git add -A src-tauri/src
git commit -m "refactor: split vibe.rs into the agents module

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 2: Registry data model with vendors, shared CLIs and metadata

**Files:**
- Modify: `src-tauri/src/agents/registry.rs`, `src-tauri/src/agents/mod.rs`, `src-tauri/src/agents/detect.rs`, `src-tauri/src/agents/install/*.rs`, `src-tauri/src/agents/activity.rs`
- Test: `src-tauri/src/agents/registry.rs` (tests module)

**Interfaces:**
- Consumes: Task 1 layout.
- Produces:
  - `registry::Vendor`, `registry::Edition`, `registry::ProductSpec`, `registry::CliSpec` (with `id`, `vendor`), `registry::DesktopSpec` (with `install_unavailable_reason`), `registry::DataDir`, `registry::DataBase`, `registry::PRODUCTS`, `registry::CLIS`.
  - `registry::ToolSpec` view fields: `id, vendor, family, edition, edition_label, sort, name, description, icon, docs_url, cli_id: Option<&'static str>, cli_note: Option<&'static str>, cli: CliSpec, desktop: DesktopSpec, desktop_available: bool, process_pattern: &'static str, data_dirs: &'static [DataDir]`.
  - `registry::spec_by_id(id) -> Option<ToolSpec>`, `registry::tool_specs() -> Vec<ToolSpec>`, `registry::cli_by_id(id) -> Option<&'static CliSpec>`.
  - `VibeTool` gains `icon: String`, `cli_id: Option<String>`, `cli_note: Option<String>`.

- [ ] **Step 1: Write the failing invariant tests**

Append to the tests module in `registry.rs`:

```rust
#[test]
fn product_and_cli_ids_are_unique() {
    let mut products = std::collections::HashSet::new();
    for product in PRODUCTS {
        assert!(products.insert(product.id), "duplicate product id {}", product.id);
    }
    let mut clis = std::collections::HashSet::new();
    for cli in CLIS {
        assert!(clis.insert(cli.id), "duplicate cli id {}", cli.id);
    }
}

#[test]
fn product_cli_references_exist() {
    for product in PRODUCTS {
        if let CliSlot::Shared(id) = product.cli {
            assert!(cli_by_id(id).is_some(), "{} references missing cli {id}", product.id);
        }
    }
}

#[test]
fn every_product_offers_a_surface_and_has_an_icon_file() {
    let brands = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../public/brands");
    for product in PRODUCTS {
        let has_cli = matches!(product.cli, CliSlot::Shared(_));
        let has_desktop = matches!(product.desktop, DesktopSlot::App(_));
        assert!(has_cli || has_desktop, "{} offers no surface", product.id);
        assert!(brands.join(product.icon).is_file(), "{} icon {} missing", product.id, product.icon);
    }
}

#[test]
fn editions_are_unique_within_a_family() {
    let mut seen = std::collections::HashSet::new();
    for product in PRODUCTS {
        assert!(seen.insert((product.family, product.edition)), "{} repeats an edition", product.id);
    }
}

#[test]
fn tool_view_resolves_shared_cli() {
    let spec = spec_by_id("codex").unwrap();
    assert_eq!(spec.cli.command, "codex");
    assert_eq!(spec.cli_id, Some("codex"));
    assert_eq!(spec.icon, "codex.png");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib agents::registry`
Expected: compile errors (`PRODUCTS`, `CLIS`, `CliSlot`, `cli_by_id` not defined).

- [ ] **Step 3: Replace the data model**

At the top of `registry.rs`, replace `CliSpec`, `DesktopSpec`, `ToolSpec`, `tool_metadata` and `tool_specs` with:

```rust
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Vendor {
    Claude,
    Codex,
    Antigravity,
    OpenCode,
    ZCode,
    Kimi,
    WorkBuddy,
    Qoder,
    Trae,
    DeepSeekHarness,
    OpenClaw,
    Hermes,
    Pi,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Edition {
    Cn,
    Global,
    Unified,
}

impl Edition {
    pub fn as_str(self) -> &'static str {
        match self {
            Edition::Cn => "cn",
            Edition::Global => "global",
            Edition::Unified => "unified",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum DataBase {
    Home,
    Roaming,
    Local,
}

/// An agent-owned data directory. Registered now for the sessions and migration sub-projects.
#[derive(Clone, Copy, Debug)]
pub struct DataDir {
    pub base: DataBase,
    pub relative: &'static str,
    pub env_override: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub struct CliSpec {
    pub id: &'static str,
    pub vendor: Vendor,
    pub name: &'static str,
    pub description: &'static str,
    pub command: &'static str,
    pub candidates: &'static [&'static str],
    pub npm_package: Option<&'static str>,
    pub winget_id: Option<&'static str>,
    pub install_url: &'static str,
    pub docs_url: &'static str,
}

#[derive(Clone, Debug)]
pub struct DesktopSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub winget_id: Option<&'static str>,
    pub winget_source: Option<&'static str>,
    pub appx_names: &'static [&'static str],
    pub install_url: &'static str,
    pub docs_url: &'static str,
    pub keywords: &'static [&'static str],
    pub excludes: &'static [&'static str],
    pub install_unavailable_reason: Option<&'static str>,
}

pub enum CliSlot {
    Shared(&'static str),
    Unavailable { name: &'static str, description: &'static str, url: &'static str },
}

pub enum DesktopSlot {
    App(DesktopSpec),
    Unavailable { name: &'static str, description: &'static str, url: &'static str },
}

pub struct ProductSpec {
    pub id: &'static str,
    pub vendor: Vendor,
    pub family: &'static str,
    pub edition: Edition,
    pub edition_label: &'static str,
    pub sort: u16,
    pub name: &'static str,
    pub description: &'static str,
    pub icon: &'static str,
    pub docs_url: &'static str,
    pub cli: CliSlot,
    pub cli_note: Option<&'static str>,
    pub desktop: DesktopSlot,
    /// PowerShell regex matched against "<process name> <command line>". Short vendor
    /// names must be anchored to a path segment or executable name.
    pub process_pattern: &'static str,
    pub data_dirs: &'static [DataDir],
}

/// Resolved view used by detection and install code.
#[derive(Clone)]
pub struct ToolSpec {
    pub id: &'static str,
    pub vendor: Vendor,
    pub family: &'static str,
    pub edition: Edition,
    pub edition_label: &'static str,
    pub sort: u16,
    pub name: &'static str,
    pub description: &'static str,
    pub icon: &'static str,
    pub docs_url: &'static str,
    pub cli_id: Option<&'static str>,
    pub cli_note: Option<&'static str>,
    pub cli: CliSpec,
    pub desktop: DesktopSpec,
    pub desktop_available: bool,
    pub process_pattern: &'static str,
    pub data_dirs: &'static [DataDir],
}

pub fn cli_by_id(id: &str) -> Option<&'static CliSpec> {
    CLIS.iter().find(|cli| cli.id == id)
}

fn resolve(product: &'static ProductSpec) -> ToolSpec {
    let (cli_id, cli) = match product.cli {
        CliSlot::Shared(id) => (Some(id), cli_by_id(id).expect("registry invariant: cli exists").clone()),
        CliSlot::Unavailable { name, description, url } => (
            None,
            CliSpec {
                id: "",
                vendor: product.vendor,
                name,
                description,
                command: "",
                candidates: &[],
                npm_package: None,
                winget_id: None,
                install_url: url,
                docs_url: url,
            },
        ),
    };
    let (desktop_available, desktop) = match &product.desktop {
        DesktopSlot::App(spec) => (true, spec.clone()),
        DesktopSlot::Unavailable { name, description, url } => (
            false,
            DesktopSpec {
                name,
                description,
                winget_id: None,
                winget_source: None,
                appx_names: &[],
                install_url: url,
                docs_url: url,
                keywords: &[],
                excludes: &[],
                install_unavailable_reason: None,
            },
        ),
    };
    ToolSpec {
        id: product.id,
        vendor: product.vendor,
        family: product.family,
        edition: product.edition,
        edition_label: product.edition_label,
        sort: product.sort,
        name: product.name,
        description: product.description,
        icon: product.icon,
        docs_url: product.docs_url,
        cli_id,
        cli_note: product.cli_note,
        cli,
        desktop,
        desktop_available,
        process_pattern: product.process_pattern,
        data_dirs: product.data_dirs,
    }
}

pub fn tool_specs() -> Vec<ToolSpec> {
    PRODUCTS.iter().map(resolve).collect()
}

pub fn spec_by_id(id: &str) -> Option<ToolSpec> {
    PRODUCTS.iter().find(|product| product.id == id).map(resolve)
}
```

- [ ] **Step 4: Fill `CLIS` and `PRODUCTS` from the existing `tool_specs` data**

Create `pub static CLIS: &[CliSpec] = &[ ... ];` and `pub static PRODUCTS: &[ProductSpec] = &[ ... ];`. Copy every field value verbatim from the old `tool_specs()` entries. Each old CLI with a non-empty `command` becomes one `CliSpec`; an old CLI with an empty `command` becomes `CliSlot::Unavailable` with the old name, description and install URL. An old desktop is `DesktopSlot::App` unless it had no keywords, no `winget_id` and no `appx_names` **and** the product is not DeepSeek Harness, in which case it is `DesktopSlot::Unavailable`. Move each `desktop_install_unavailable_reason` match arm text into that product's `DesktopSpec.install_unavailable_reason` (products not listed there get `None`).

Metadata for the products that exist today (the WorkBuddy and Qoder rows are replaced in Task 3):

| id | vendor | family | edition | edition_label | sort | icon | cli slot | process_pattern |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `claude` | Claude | claude | Global | "" | 10 | `claude.png` | Shared(`claude`) | `(?i)(^\|[\\/\s"])claude([\\/\s".]\|$)\|@anthropic-ai[\\/]claude-code` |
| `codex` | Codex | codex | Global | "" | 20 | `codex.png` | Shared(`codex`) | `(?i)(^\|[\\/\s"])codex([\\/\s".]\|$)\|@openai[\\/]codex` |
| `antigravity` | Antigravity | antigravity | Global | "" | 30 | `antigravity.png` | Shared(`agy`) | `(?i)antigravity\|(^\|[\\/])agy(\\.cmd\|\\.exe)?` |
| `opencode` | OpenCode | opencode | Global | "" | 40 | `opencode-icon.png` | Shared(`opencode`) | `(?i)opencode` |
| `zcode` | ZCode | zcode | Global | "" | 50 | `zcode.svg` | Unavailable | `(?i)zcode\|z\.ai` |
| `kimi` | Kimi | kimi | Unified | "" | 60 | `kimi.ico` | Shared(`kimi`) | `(?i)(^\|[\\/\s"])kimi(-cli\|-code)?([\\/\s".]\|$)` |
| `trae-work` | Trae | trae | Cn | "中国版" | 90 | `trae-work.png` | Shared(`traecli`) | `(?i)(^\|[\\/\s"])trae([\\/\s".]\|$)` |
| `trae-global` | Trae | trae | Global | "国际版" | 91 | `trae-work.png` | Unavailable | `(?i)(^\|[\\/\s"])trae([\\/\s".]\|$)` |
| `deepseek-harness` | DeepSeekHarness | deepseek-harness | Global | "开发者预览" | 100 | `deepseek.svg` | Shared(`dsh`) | `(?i)deepseek-harness\|@deepseek-ai[\\/]dsh\|(^\|[\\/])dsh(\.cmd\|\.exe)?` |
| `openclaw` | OpenClaw | openclaw | Global | "" | 110 | `openclaw.svg` | Shared(`openclaw`) | `(?i)openclaw` |
| `hermes` | Hermes | hermes | Global | "" | 120 | `hermes.png` | Shared(`hermes`) | `(?i)(^\|[\\/\s"])hermes(-agent)?([\\/\s".]\|$)` |

(The `\|` in the table is a literal `|` in the Rust string. Copy the patterns from the current PowerShell block in `activity.rs`; they are the phase-0 patterns.)

`data_dirs` for this task:

```rust
const CLAUDE_DATA: &[DataDir] = &[
    DataDir { base: DataBase::Home, relative: ".claude", env_override: Some("CLAUDE_CONFIG_DIR") },
    DataDir { base: DataBase::Roaming, relative: "Claude", env_override: None },
];
const CODEX_DATA: &[DataDir] = &[
    DataDir { base: DataBase::Home, relative: ".codex", env_override: Some("CODEX_HOME") },
];
```

Every other product uses the directories currently listed for it in `work_session.rs::agent_data_roots` (home-relative entries → `DataBase::Home`, `roaming.join(..)` → `DataBase::Roaming`, `local.join(..)` → `DataBase::Local`, `env_override: None`).

- [ ] **Step 5: Replace `tool_metadata` users and `match spec.id` dispatch with vendor dispatch**

- In `agents/mod.rs`, `vibe_catalog_tool` and `vibe_tool_from_spec` read `spec.family`, `spec.edition.as_str()`, `spec.edition_label`, `spec.sort` and fill the new `VibeTool` fields:

```rust
#[derive(Serialize, Clone)]
pub struct VibeTool {
    pub id: String,
    pub family_id: String,
    pub edition: String,
    pub edition_label: String,
    pub sort_order: u16,
    pub name: String,
    pub description: String,
    pub docs_url: String,
    pub icon: String,
    pub cli_id: Option<String>,
    pub cli_note: Option<String>,
    pub cli: VibeSurface,
    pub desktop: VibeSurface,
}
```

- Replace every `spec.id == "<x>"` and `match spec.id { "<x>" => ... }` in `detect.rs`, `install/*.rs` and `mod.rs` with the matching `spec.vendor == Vendor::X` / `match spec.vendor { Vendor::X => ... }`. Id-to-vendor: `claude`→Claude, `codex`→Codex, `antigravity`→Antigravity, `opencode`→OpenCode, `zcode`→ZCode, `kimi`→Kimi, `workbuddy`→WorkBuddy, `qoder`→Qoder, `trae-work`→Trae, `deepseek-harness`→DeepSeekHarness, `openclaw`→OpenClaw, `hermes`→Hermes.
- `direct_desktop_installer(id: &str)` becomes `direct_desktop_installer(vendor: Vendor)`.
- `desktop_install_unavailable_reason(spec)` becomes `spec.desktop.install_unavailable_reason.unwrap_or("官方未提供可自动安装的独立 Windows 应用。")`.
- The desktop-availability checks in `vibe_catalog_tool` and `desktop_surface` (`spec.id != "deepseek-harness" && spec.desktop.keywords.is_empty() && ...`) become `!spec.desktop_available`.
- `download_desktop_installer` keeps using `spec.id` only inside the temp file name.

- [ ] **Step 6: Run the registry tests and the full Rust check**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib agents::registry` then the full check from Global Constraints.
Expected: the five new tests pass; all previous tests pass.

- [ ] **Step 7: Commit**

```bash
git add -A src-tauri/src/agents
git commit -m "refactor: drive the agent catalog from a declarative registry

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 3: Catalog changes — WorkBuddy CN/global, Qoder new line, pi

**Files:**
- Modify: `src-tauri/src/agents/registry.rs`, `src-tauri/src/agents/detect.rs` (`desktop_candidate_paths`), `src/features/..` none yet
- Test: `src-tauri/src/agents/registry.rs`

**Interfaces:**
- Produces: product ids `workbuddy-cn`, `workbuddy-global` (shared cli `codebuddy`), `qoder` and `qoder-cn` (shared cli `qodercli`), `pi` (cli `pi`). The old product id `workbuddy` no longer exists.

- [ ] **Step 1: Verify vendor facts before writing data**

Use WebFetch on each page and record in the commit message what you found:
- `https://www.workbuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Installation-Win-Guide` and `https://www.codebuddy.cn/work/`: Windows download URL for the CN desktop app, install location, uninstall display name, executable name.
- `https://www.workbuddy.ai/docs/workbuddy/From-Beginner-to-Expert-Guide/Installation-Win-Guide`: the same for the global desktop app.
- `https://qoder.com/en/download` and `https://qoder.com.cn/download`: new Qoder desktop product name, executable name and uninstall display name for each edition, and how Qoder IDE is named (for exclusion).
- `https://www.npmjs.com/package/@mariozechner/pi-coding-agent`: command name and `--version` behavior.

If a desktop download cannot be installed silently from a stable URL, leave `winget_id: None`, add no direct installer, and set `install_unavailable_reason: Some("尚未找到可稳定调用的官方 Windows 安装接口，请通过官方下载页安装。")`.

- [ ] **Step 2: Write the failing catalog tests**

```rust
#[test]
fn workbuddy_editions_share_the_codebuddy_cli() {
    let cn = spec_by_id("workbuddy-cn").unwrap();
    let global = spec_by_id("workbuddy-global").unwrap();
    assert_eq!(cn.cli_id, Some("codebuddy"));
    assert_eq!(global.cli_id, Some("codebuddy"));
    assert_eq!(cn.edition, Edition::Cn);
    assert_eq!(global.edition, Edition::Global);
    assert!(cn.cli_note.unwrap().contains("中国站"));
    assert!(spec_by_id("workbuddy").is_none());
}

#[test]
fn qoder_editions_share_one_cli_and_exclude_the_ide() {
    for id in ["qoder", "qoder-cn"] {
        let spec = spec_by_id(id).unwrap();
        assert_eq!(spec.cli_id, Some("qodercli"));
        assert!(spec.desktop.excludes.iter().any(|value| value.contains("ide")), "{id}");
    }
}

#[test]
fn pi_is_an_npm_cli_without_desktop() {
    let pi = spec_by_id("pi").unwrap();
    assert_eq!(pi.cli.command, "pi");
    assert_eq!(pi.cli.npm_package, Some("@mariozechner/pi-coding-agent"));
    assert!(!pi.desktop_available);
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib agents::registry`
Expected: FAIL — `workbuddy-cn`, `qodercli`, `pi` missing.

- [ ] **Step 4: Update the registry**

- Rename the old `workbuddy` CLI entry's id to `codebuddy` (keep its candidates and npm package).
- Replace the `workbuddy` product with two products:

```rust
ProductSpec {
    id: "workbuddy-cn",
    vendor: Vendor::WorkBuddy,
    family: "workbuddy",
    edition: Edition::Cn,
    edition_label: "中国版",
    sort: 70,
    name: "WorkBuddy 中国版",
    description: "腾讯 WorkBuddy 中国站桌面工作智能体，配套 CodeBuddy CLI。",
    icon: "workbuddy.svg",
    docs_url: "https://www.workbuddy.cn/docs/workbuddy/Quickstart",
    cli: CliSlot::Shared("codebuddy"),
    cli_note: Some("与国际版共用 CodeBuddy CLI，登录时选择中国站。"),
    desktop: DesktopSlot::App(DesktopSpec {
        name: "WorkBuddy 桌面端（中国版）",
        description: "腾讯 WorkBuddy 中国站 Windows 桌面应用。",
        winget_id: None,
        winget_source: None,
        appx_names: &[],
        install_url: "https://www.workbuddy.cn/docs/workbuddy/From-Beginner-to-Expert-Guide/Installation-Win-Guide",
        docs_url: "https://www.workbuddy.cn/docs/workbuddy/Quickstart",
        keywords: &[/* CN uninstall display name found in Step 1, lowercase */],
        excludes: &[/* global display name found in Step 1, lowercase */],
        install_unavailable_reason: Some("尚未找到可稳定调用的官方 Windows 安装接口，请通过官方下载页安装。"),
    }),
    process_pattern: r"(?i)workbuddy",
    data_dirs: &[DataDir { base: DataBase::Home, relative: ".workbuddy", env_override: None }, DataDir { base: DataBase::Roaming, relative: "WorkBuddy", env_override: None }],
},
```

and `workbuddy-global` with `edition: Edition::Global`, `edition_label: "国际版"`, `sort: 71`, `name: "WorkBuddy 国际版"`, workbuddy.ai URLs, `cli_note: None`, and keywords/excludes swapped from Step 1. If Step 1 shows both editions register the same display name, use the install directory recorded in Step 1 to tell them apart by adding it to `desktop_candidate_paths` (match on `spec.id` there only for these two ids) and leave `keywords` identical.

- Replace the two old Qoder CLI entries with one `CliSpec { id: "qodercli", vendor: Vendor::Qoder, name: "Qoder CLI", command: "qoder", candidates: <old list>, npm_package: Some("@qoder-ai/qodercli"), .. }`. Both Qoder products use `CliSlot::Shared("qodercli")`; `qoder-cn` gets `cli_note: Some("与国际版共用同一个 qodercli 命令。")`. Set each desktop's `name`, `description`, `keywords` to the new Qoder product line from Step 1, and add `"qoder ide"` plus the IDE display names found in Step 1 to `excludes`.
- Add:

```rust
CliSpec {
    id: "pi",
    vendor: Vendor::Pi,
    name: "pi",
    description: "Mario Zechner 的极简终端编程智能体，命令名 pi。",
    command: "pi",
    candidates: &["pi.cmd", "pi.exe", "pi.bat", "pi.ps1"],
    npm_package: Some("@mariozechner/pi-coding-agent"),
    winget_id: None,
    install_url: "https://pi.dev/",
    docs_url: "https://pi.dev/",
},
```

```rust
ProductSpec {
    id: "pi",
    vendor: Vendor::Pi,
    family: "pi",
    edition: Edition::Global,
    edition_label: "",
    sort: 130,
    name: "pi",
    description: "极简开源终端编程智能体，内置读、写、编辑、命令四种工具，可通过扩展和技能定制。",
    icon: "pi.svg",
    docs_url: "https://pi.dev/",
    cli: CliSlot::Shared("pi"),
    cli_note: None,
    desktop: DesktopSlot::Unavailable {
        name: "pi 桌面端",
        description: "pi 只提供终端使用方式。",
        url: "https://pi.dev/",
    },
    process_pattern: r#"(?i)@mariozechner[\\/]pi-coding-agent|(^|[\\/\s"])pi(\.cmd|\.exe)([\s"]|$)"#,
    data_dirs: &[DataDir { base: DataBase::Home, relative: ".pi", env_override: None }],
},
```

- Add `public/brands/pi.svg`: a 64×64 SVG with a rounded square (`rx="14"`, fill `#111`) and a centered white "π" glyph (`font-family="Georgia, serif"`, `font-size="40"`).

- [ ] **Step 5: Run the tests and the full Rust check**

Expected: all registry tests pass, including the invariants from Task 2.

- [ ] **Step 6: Commit** with the Step 1 findings in the body.

```bash
git add -A src-tauri/src/agents public/brands/pi.svg
git commit -m "feat: split WorkBuddy editions, move Qoder to its new line, add pi

<Step 1 findings: download URLs, display names, executables>

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 4: Generate the running-agent scan from the registry

**Files:**
- Modify: `src-tauri/src/agents/activity.rs`
- Test: same file

**Interfaces:**
- Consumes: `registry::PRODUCTS` (`family`, `name`, `process_pattern`).
- Produces: `activity::process_patterns() -> Vec<(String /*family*/, String /*display name*/, String /*pattern*/)>`, one row per family; `scan_agent_activity()` reports `agent_id = family`. `desktop_agent_processes(id)` filters by the product's `family`. `activity_family` is deleted.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn one_pattern_per_family() {
    let patterns = process_patterns();
    let families: std::collections::HashSet<_> = patterns.iter().map(|row| row.0.clone()).collect();
    assert_eq!(families.len(), patterns.len());
    assert!(families.contains("workbuddy"));
    assert!(families.contains("trae"));
}

#[test]
fn powershell_rows_escape_single_quotes() {
    let script = patterns_powershell(&[("x".into(), "X's".into(), "(?i)a'b".into())]);
    assert!(script.contains("Name = 'X''s'"));
    assert!(script.contains("Pattern = '(?i)a''b'"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib agents::activity`
Expected: FAIL — functions missing.

- [ ] **Step 3: Implement**

```rust
pub(crate) fn process_patterns() -> Vec<(String, String, String)> {
    let mut rows: Vec<(String, String, String)> = Vec::new();
    for product in super::registry::PRODUCTS {
        if rows.iter().any(|row| row.0 == product.family) {
            continue;
        }
        let name = product.name.split_whitespace().next().unwrap_or(product.name);
        rows.push((product.family.into(), name.into(), product.process_pattern.into()));
    }
    rows
}

fn patterns_powershell(rows: &[(String, String, String)]) -> String {
    let quote = |value: &str| value.replace('\'', "''");
    let body = rows
        .iter()
        .map(|(id, name, pattern)| {
            format!(
                "  @{{ Id = '{}'; Name = '{}'; Pattern = '{}' }}",
                quote(id),
                quote(name),
                quote(pattern)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!("$patterns = @(\n{body}\n)")
}
```

In `scan_agent_activity`, replace the hard-coded `$patterns = @( ... )` block of the PowerShell script with `patterns_powershell(&process_patterns())` (build the script with `format!`, keeping the rest of the script text unchanged; the script currently lives in a raw string, so split it into a prefix and suffix around the patterns block). In `desktop_agent_processes`, replace `activity_family(id)` with `super::registry::spec_by_id(id).map(|spec| spec.family).ok_or("Unknown work agent.")?` and delete `activity_family`.

- [ ] **Step 4: Run tests and the full Rust check.** Expected: pass.

- [ ] **Step 5: Commit** — `feat: build running-agent detection from the registry`.

---

### Task 5: Health-checked CLI detection

**Files:**
- Create: `src-tauri/src/agents/health.rs`
- Modify: `src-tauri/src/agents/mod.rs` (`VibeSurface`), `src-tauri/src/agents/detect.rs` (`cli_surface`, `desktop_surface`), `src-tauri/src/agents/process.rs` (expose `command_dirs`)
- Test: `src-tauri/src/agents/health.rs`

**Interfaces:**
- Produces:
  - `health::InstallInfo { path: String, healthy: bool, version: Option<String>, reason: Option<String> }` (`Serialize`, camelCase).
  - `health::has_pe_header(path: &Path) -> bool`
  - `health::enumerate_candidates(dirs: &[PathBuf], candidates: &[&str]) -> Vec<PathBuf>` — PATH order, first candidate per directory, skips `\WindowsApps`.
  - `health::check_install(path: &Path, probe: &dyn Fn(&Path) -> Result<String, String>) -> InstallInfo`
  - `VibeSurface` new fields: `health: String` (`"healthy" | "broken" | "missing"`), `broken_reason: Option<String>`, `other_installs: Vec<InstallInfo>`, `can_repair: bool`. `status` may now be `"broken"`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn exe_without_mz_header_is_broken_without_running_it() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("claude.exe");
        fs::write(&exe, b"#!/bin/sh\necho native binary not installed\n").unwrap();
        let info = check_install(&exe, &|_| panic!("must not execute a non-PE file"));
        assert!(!info.healthy);
        assert!(info.reason.unwrap().contains("不是有效的 Windows 程序"));
    }

    #[test]
    fn exe_with_mz_header_is_probed() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("tool.exe");
        fs::write(&exe, b"MZ\x90\x00").unwrap();
        let info = check_install(&exe, &|_| Ok("1.2.3".into()));
        assert!(info.healthy);
        assert_eq!(info.version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn failed_probe_is_broken_with_reason() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = dir.path().join("tool.cmd");
        fs::write(&cmd, b"@echo off").unwrap();
        let info = check_install(&cmd, &|_| Err("不支持的 16 位应用程序".into()));
        assert!(!info.healthy);
        assert_eq!(info.reason.as_deref(), Some("不支持的 16 位应用程序"));
    }

    #[test]
    fn candidates_follow_path_order_one_per_directory() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        for dir in [first.path(), second.path()] {
            fs::write(dir.join("tool.cmd"), b"").unwrap();
            fs::write(dir.join("tool.ps1"), b"").unwrap();
        }
        let found = enumerate_candidates(
            &[first.path().to_path_buf(), second.path().to_path_buf(), first.path().to_path_buf()],
            &["tool.cmd", "tool.ps1"],
        );
        assert_eq!(found, vec![first.path().join("tool.cmd"), second.path().join("tool.cmd")]);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib agents::health`
Expected: compile error — module missing. Add `mod health;` to `agents/mod.rs` first so the failure is "function not found".

- [ ] **Step 3: Implement `health.rs`**

```rust
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallInfo {
    pub path: String,
    pub healthy: bool,
    pub version: Option<String>,
    pub reason: Option<String>,
}

/// npm placeholder "binaries" are shell scripts named `.exe`; real PE files start with `MZ`.
pub fn has_pe_header(path: &Path) -> bool {
    let mut header = [0u8; 2];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok()
        && &header == b"MZ"
}

pub fn enumerate_candidates(dirs: &[PathBuf], candidates: &[&str]) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut found = Vec::new();
    for dir in dirs {
        let key = dir.to_string_lossy().trim_end_matches('\\').to_lowercase();
        if key.contains("\\windowsapps") || !seen.insert(key) {
            continue;
        }
        if let Some(path) = candidates.iter().map(|name| dir.join(name)).find(|path| path.is_file()) {
            found.push(path);
        }
    }
    found
}

pub fn check_install(path: &Path, probe: &dyn Fn(&Path) -> Result<String, String>) -> InstallInfo {
    let display = path.to_string_lossy().into_owned();
    let is_exe = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
    if is_exe && !has_pe_header(path) {
        return InstallInfo {
            path: display,
            healthy: false,
            version: None,
            reason: Some("不是有效的 Windows 程序（可能是安装失败留下的占位文件）".into()),
        };
    }
    match probe(path) {
        Ok(version) => InstallInfo { path: display, healthy: true, version: Some(version), reason: None },
        Err(reason) => InstallInfo { path: display, healthy: false, version: None, reason: Some(reason) },
    }
}
```

- [ ] **Step 4: Use it in `cli_surface`**

Replace the `resolve_command` + single probe in `detect.rs::cli_surface` with:

```rust
let probe = |path: &Path| run_program_probe(spec.cli.name, path, &["--version"], Duration::from_secs(15));
let mut installs = super::health::enumerate_candidates(&command_dirs(), spec.cli.candidates)
    .into_iter()
    .map(|path| super::health::check_install(&path, &probe));
let effective = installs.next();
let other_installs: Vec<_> = installs.collect();
let program = effective.as_ref().map(|info| PathBuf::from(&info.path));
let healthy = effective.as_ref().is_some_and(|info| info.healthy);
let version = effective.as_ref().and_then(|info| info.version.clone());
let broken_reason = effective.as_ref().filter(|info| !info.healthy).and_then(|info| info.reason.clone());
let method = detect_install_method(spec, program.as_deref());
let installed = healthy;
```

Then compute `latest`/`update_available` exactly as today using `installed`, and set:

```rust
let health = match &effective {
    None => "missing",
    Some(info) if info.healthy => "healthy",
    Some(_) => "broken",
};
let status = if health == "broken" {
    "broken"
} else if update_available {
    "update"
} else if installed {
    "installed"
} else {
    "missing"
};
let can_repair = health == "broken" && other_installs.iter().any(|info| info.healthy);
```

Fill the new `VibeSurface` fields; `probe_error` becomes `broken_reason.clone()`; `can_update: installed`, `can_uninstall: program.is_some()`, `can_open: installed`. Add the four new fields with `"missing"`/`None`/`Vec::new()`/`false` defaults to `pending_surface` and `unavailable_surface`. In `desktop_surface`, set `health` to `"healthy"` when found, else `"missing"`; if the found path ends with `.exe` and `!has_pe_header`, set `health: "broken"`, `status: "broken"`, `installed: false`, and `broken_reason: Some("不是有效的 Windows 程序".into())`.

- [ ] **Step 5: Run tests and the full Rust check.** Expected: pass.

- [ ] **Step 6: Commit** — `feat: health-check agent CLIs instead of trusting file existence`.

---

### Task 6: Proxy injection rules for agent actions

**Files:**
- Create: `src-tauri/src/agents/net.rs`
- Modify: `src-tauri/src/agents/install/winget.rs` (`winget_args`, delete `configured_proxy_url`), `src-tauri/src/agents/install/direct.rs` (`download_desktop_installer`, `desktop_download_agent`), `src-tauri/src/agents/install/npm.rs` (`validate_npm_proxy`), `src-tauri/src/agents/process.rs` (`run_command_streamed`), `src-tauri/src/settings.rs` (make `detected_proxy_addr` `pub(crate)`)
- Test: `src-tauri/src/agents/net.rs`

**Interfaces:**
- Produces: `net::stacker_proxy() -> Option<String>` (full URL such as `http://127.0.0.1:7890`), `net::select_proxy(mode: &str, manual: (&str, u16), system: Option<(String, u16)>) -> Option<String>`, `net::proxy_env(proxy: &str) -> [(&'static str, String); 4]`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_mode_never_injects() {
        assert_eq!(select_proxy("off", ("127.0.0.1", 7890), Some(("10.0.0.1".into(), 8080))), None);
    }

    #[test]
    fn manual_mode_uses_the_stacker_address() {
        assert_eq!(select_proxy("manual", ("127.0.0.1", 7890), None).as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(select_proxy("manual", ("", 0), None), None);
    }

    #[test]
    fn system_mode_uses_the_current_windows_proxy_only() {
        assert_eq!(select_proxy("system", ("", 0), Some(("10.0.0.1".into(), 8080))).as_deref(), Some("http://10.0.0.1:8080"));
        assert_eq!(select_proxy("system", ("127.0.0.1", 7890), None), None);
    }

    #[test]
    fn env_covers_npm_and_generic_clients() {
        let keys: Vec<_> = proxy_env("http://h:1").iter().map(|(key, _)| *key).collect();
        assert_eq!(keys, ["HTTP_PROXY", "HTTPS_PROXY", "npm_config_proxy", "npm_config_https_proxy"]);
    }
}
```

- [ ] **Step 2: Run to verify failure.** Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib agents::net`. Expected: FAIL.

- [ ] **Step 3: Implement `net.rs`**

```rust
//! Proxy used by Stacker's own agent downloads and the installer processes it starts.
//! It never reads or writes the user's environment variables or tool configs.

pub fn select_proxy(mode: &str, manual: (&str, u16), system: Option<(String, u16)>) -> Option<String> {
    match mode {
        "manual" if !manual.0.trim().is_empty() && manual.1 > 0 => {
            Some(format!("http://{}:{}", manual.0.trim(), manual.1))
        }
        "system" => system.map(|(host, port)| format!("http://{host}:{port}")),
        _ => None,
    }
}

pub fn stacker_proxy() -> Option<String> {
    let (host, port) = crate::settings::proxy_addr();
    select_proxy(
        &crate::settings::proxy_mode(),
        (&host, port),
        crate::settings::detected_proxy_addr(),
    )
}

pub fn proxy_env(proxy: &str) -> [(&'static str, String); 4] {
    [
        ("HTTP_PROXY", proxy.to_string()),
        ("HTTPS_PROXY", proxy.to_string()),
        ("npm_config_proxy", proxy.to_string()),
        ("npm_config_https_proxy", proxy.to_string()),
    ]
}
```

- [ ] **Step 4: Wire it in**

- `winget.rs`: `winget_args` passes `super::super::net::stacker_proxy().as_deref()`; delete `configured_proxy_url`.
- `direct.rs`: `desktop_download_agent` takes `proxy: Option<&str>`; when `Some`, build `ureq::Proxy::new(proxy)`. In `download_desktop_installer`, compute `let proxy = super::super::net::stacker_proxy();`, emit `正在通过 Stacker 代理 {proxy} 连接官方下载地址…` when `Some`, and drop the `crate::proxy::status()` call.
- `process.rs::run_command_streamed`: after `apply_fresh_path(&mut cmd);` add:

```rust
if let Some(proxy) = super::net::stacker_proxy() {
    for (key, value) in super::net::proxy_env(&proxy) {
        cmd.env(key, value);
    }
}
```

- `npm.rs::validate_npm_proxy`: delete the `proxy_mode` block that calls `settings_set_proxy_mode` / `sync_existing_explicit_proxies`. Keep the read-only reachability check, but skip it when `stacker_proxy()` is `Some` (the injected env overrides npm config). Change its error text to: `npm 的 {key} 配置指向本机代理 {proxy}，但该端口当前不可连接。Stacker 不会修改 npm 配置，请在终端执行 npm config delete {key} 或恢复该代理后重试。`
- `settings.rs`: `fn detected_proxy_addr` → `pub(crate) fn detected_proxy_addr`.

- [ ] **Step 5: Run tests and the full Rust check.** Expected: pass. Also run `rg -n "proxy::status\(\)" src-tauri/src/agents` — expected: no matches.

- [ ] **Step 6: Commit** — `fix: inject only Stacker-selected proxies into agent installs`.

---

### Task 7: Per-task cancellation and log routing

**Files:**
- Modify: `src-tauri/src/installer.rs:30-50`, `src-tauri/src/agents/mod.rs` (`emit_progress`), `src-tauri/src/agents/process.rs` (`run_command_streamed` reader threads)
- Test: `src-tauri/src/installer.rs`

**Interfaces:**
- Produces in `crate::installer`:
  - `#[derive(Clone)] pub struct TaskContext { pub cancel: Arc<AtomicBool>, pub log: Arc<dyn Fn(&str) + Send + Sync> }`
  - `pub fn with_task_context<R>(context: TaskContext, work: impl FnOnce() -> R) -> R`
  - `pub fn current_task_context() -> Option<TaskContext>`
  - `pub fn task_log(line: &str) -> bool`
  - `op_cancelled()` / `op_reset()` keep their signatures.

- [ ] **Step 1: Write the failing tests** (append a tests module to `installer.rs`, or extend the existing one)

```rust
#[test]
fn task_context_isolates_cancellation_and_logs() {
    use std::sync::{Arc, Mutex};
    use std::sync::atomic::AtomicBool;

    let cancel = Arc::new(AtomicBool::new(false));
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = lines.clone();
    let context = TaskContext {
        cancel: cancel.clone(),
        log: Arc::new(move |line: &str| sink.lock().unwrap().push(line.to_string())),
    };
    op_cancel();
    let observed = with_task_context(context, || {
        assert!(!op_cancelled(), "global cancel must not leak into a task");
        assert!(task_log("hello"));
        cancel.store(true, Ordering::SeqCst);
        op_cancelled()
    });
    assert!(observed);
    assert_eq!(*lines.lock().unwrap(), vec!["hello".to_string()]);
    assert!(op_cancelled(), "outside the task the global flag applies");
    assert!(!task_log("ignored"));
    op_reset();
}
```

- [ ] **Step 2: Run to verify failure.** Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib installer::tests::task_context`. Expected: FAIL — `TaskContext` missing.

- [ ] **Step 3: Implement** (replace `op_reset` / `op_cancelled` in `installer.rs`)

```rust
use std::cell::RefCell;
use std::sync::Arc;

#[derive(Clone)]
pub struct TaskContext {
    pub cancel: Arc<AtomicBool>,
    pub log: Arc<dyn Fn(&str) + Send + Sync>,
}

thread_local! {
    static TASK_CONTEXT: RefCell<Option<TaskContext>> = const { RefCell::new(None) };
}

/// Runs `work` with a task-scoped cancel flag and log sink. Installer helpers called inside
/// it report to this task instead of the single global operation.
pub fn with_task_context<R>(context: TaskContext, work: impl FnOnce() -> R) -> R {
    struct Reset(Option<TaskContext>);
    impl Drop for Reset {
        fn drop(&mut self) {
            let previous = self.0.take();
            TASK_CONTEXT.with(|slot| *slot.borrow_mut() = previous);
        }
    }
    let previous = TASK_CONTEXT.with(|slot| slot.borrow_mut().replace(context));
    let _reset = Reset(previous);
    work()
}

pub fn current_task_context() -> Option<TaskContext> {
    TASK_CONTEXT.with(|slot| slot.borrow().clone())
}

pub fn task_log(line: &str) -> bool {
    match current_task_context() {
        Some(context) => {
            (context.log)(line);
            true
        }
        None => false,
    }
}

pub fn op_reset() {
    if current_task_context().is_none() {
        OP_CANCEL.store(false, Ordering::SeqCst);
    }
}

pub fn op_cancelled() -> bool {
    match current_task_context() {
        Some(context) => context.cancel.load(Ordering::SeqCst),
        None => OP_CANCEL.load(Ordering::SeqCst),
    }
}
```

- [ ] **Step 4: Route agent progress**

In `agents/mod.rs`:

```rust
fn emit_progress<S: AsRef<str>>(window: &Option<tauri::Window>, msg: S) {
    if crate::installer::task_log(msg.as_ref()) {
        return;
    }
    if let Some(window) = window {
        let _ = window.emit(VIBE_PROGRESS_EVENT, msg.as_ref().to_string());
    }
}
```

In `process.rs::run_command_streamed`, capture `let task = crate::installer::current_task_context();` before `spawn_reader`, clone it into each reader closure, and wrap the reader thread body:

```rust
std::thread::spawn(move || {
    let read = move || { /* existing loop body unchanged */ };
    match task {
        Some(context) => crate::installer::with_task_context(context, read),
        None => read(),
    }
})
```

- [ ] **Step 5: Run tests and the full Rust check.** Expected: pass.

- [ ] **Step 6: Commit** — `feat: scope installer cancellation and progress to a task`.

---

### Task 8: Resource scheduler

**Files:**
- Create: `src-tauri/src/agents/tasks/mod.rs` (declares `pub mod schedule;` for now), `src-tauri/src/agents/tasks/schedule.rs`
- Modify: `src-tauri/src/agents/mod.rs` (`pub mod tasks;`)
- Test: `src-tauri/src/agents/tasks/schedule.rs`

**Interfaces:**
- Produces: `schedule::Resource { Product(String), Npm, Installer, Download }` (`Clone, Debug, PartialEq, Eq, Hash`), `schedule::runnable(queued: &[Vec<Resource>], running: &[Vec<Resource>]) -> Vec<usize>`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use Resource::*;

    fn product(id: &str) -> Resource {
        Product(id.into())
    }

    #[test]
    fn npm_tasks_run_one_at_a_time() {
        let queued = vec![vec![product("a"), Npm], vec![product("b"), Npm]];
        assert_eq!(runnable(&queued, &[]), vec![0]);
    }

    #[test]
    fn unrelated_tasks_run_together() {
        let queued = vec![vec![product("a"), Npm], vec![product("b"), Installer, Download]];
        assert_eq!(runnable(&queued, &[]), vec![0, 1]);
    }

    #[test]
    fn at_most_three_downloads() {
        let queued: Vec<_> = ["a", "b", "c", "d"].iter().map(|id| vec![product(id), Download]).collect();
        assert_eq!(runnable(&queued, &[]), vec![0, 1, 2]);
    }

    #[test]
    fn a_blocked_task_does_not_block_later_ones() {
        let running = vec![vec![product("a"), Download]];
        let queued = vec![vec![product("a"), Download], vec![product("b"), Download]];
        assert_eq!(runnable(&queued, &running), vec![1]);
    }

    #[test]
    fn released_resources_wake_queued_tasks() {
        let queued = vec![vec![product("a"), Installer]];
        assert!(runnable(&queued, &[vec![product("x"), Installer]]).is_empty());
        assert_eq!(runnable(&queued, &[]), vec![0]);
    }
}
```

- [ ] **Step 2: Run to verify failure.** Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib agents::tasks::schedule`. Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Resource {
    /// A product id or shared CLI id; its CLI and desktop never change concurrently.
    Product(String),
    /// The single npm global directory.
    Npm,
    /// WinGet, MSI and vendor installers, which lock each other out.
    Installer,
    Download,
}

impl Resource {
    fn capacity(&self) -> usize {
        match self {
            Resource::Download => 3,
            _ => 1,
        }
    }
}

/// Indexes of queued tasks that can start now, in queue order. A task that does not fit
/// is skipped without blocking later tasks.
pub fn runnable(queued: &[Vec<Resource>], running: &[Vec<Resource>]) -> Vec<usize> {
    let mut used: HashMap<Resource, usize> = HashMap::new();
    for resource in running.iter().flatten() {
        *used.entry(resource.clone()).or_default() += 1;
    }
    let mut start = Vec::new();
    for (index, resources) in queued.iter().enumerate() {
        let fits = resources
            .iter()
            .all(|resource| used.get(resource).copied().unwrap_or(0) < resource.capacity());
        if fits {
            for resource in resources {
                *used.entry(resource.clone()).or_default() += 1;
            }
            start.push(index);
        }
    }
    start
}
```

- [ ] **Step 4: Run tests and the full Rust check.** Expected: pass.

- [ ] **Step 5: Commit** — `feat: add the agent task resource scheduler`.

---

### Task 9: Task manager, runner, post-check, commands and quit prompt

**Files:**
- Modify: `src-tauri/src/agents/tasks/mod.rs`
- Create: `src-tauri/src/agents/tasks/runner.rs`
- Modify: `src-tauri/src/agents/commands.rs`, `src-tauri/src/agents/mod.rs` (`run_tool_action` gains `"repair"`), `src-tauri/src/agents/install/mod.rs` (repair), `src-tauri/src/lib.rs` (manage state, register commands, quit prompt)
- Test: `src-tauri/src/agents/tasks/mod.rs`, `src-tauri/src/agents/tasks/runner.rs`

**Interfaces:**
- Consumes: `schedule::{Resource, runnable}`, `installer::{TaskContext, with_task_context}`, `registry::spec_by_id`, `agents::scan_vibe_tool`, `agents::run_tool_action`.
- Produces:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface { Cli, Desktop }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action { Install, Update, Uninstall, Repair }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskState { Queued, Running, Succeeded, Failed, Cancelled }

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRequest { pub product_id: String, pub surface: Surface, pub action: Action }

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTask {
    pub id: String,
    pub product_id: String,
    pub product_name: String,
    pub surface: Surface,
    pub surface_label: String,
    pub cli_id: Option<String>,
    pub action: Action,
    pub state: TaskState,
    pub message: Option<String>,
    pub last_line: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

pub struct TaskPlan { pub key: String, pub product_name: String, pub surface_label: String, pub cli_id: Option<String>, pub resources: Vec<Resource> }

pub trait TaskRunner: Send + Sync + 'static {
    fn plan(&self, request: &TaskRequest) -> Result<TaskPlan, String>;
    fn run(&self, request: &TaskRequest) -> Result<String, String>;
}

pub struct AgentTaskManager { /* private */ }
impl AgentTaskManager {
    pub fn new(runner: Arc<dyn TaskRunner>, emit: Arc<dyn Fn(&AgentTask) + Send + Sync>) -> Self;
    pub fn start(&self, request: TaskRequest) -> Result<AgentTask, String>;
    pub fn cancel(&self, id: &str) -> Result<(), String>;
    pub fn retry(&self, id: &str) -> Result<AgentTask, String>;
    pub fn list(&self) -> Vec<AgentTask>;
    pub fn log(&self, id: &str) -> Result<Vec<String>, String>;
    pub fn running_count(&self) -> usize;
    pub fn cancel_all(&self);
}
```

  - `runner::verify_outcome(action: Action, before_version: Option<&str>, after: &VibeSurface) -> Result<(), String>`
  - `runner::ProductionRunner` implements `TaskRunner`.
  - Tauri commands: `agent_task_start(request: TaskRequest) -> AgentTask`, `agent_task_cancel(id)`, `agent_task_retry(id) -> AgentTask`, `agent_tasks() -> Vec<AgentTask>`, `agent_task_log(id) -> Vec<String>`. Event name: `agent-task`, payload `AgentTask`.

- [ ] **Step 1: Write the failing manager tests** (in `tasks/mod.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    struct FakeRunner {
        gate: Mutex<mpsc::Receiver<Result<String, String>>>,
    }

    impl TaskRunner for FakeRunner {
        fn plan(&self, request: &TaskRequest) -> Result<TaskPlan, String> {
            Ok(TaskPlan {
                key: request.product_id.clone(),
                product_name: request.product_id.clone(),
                surface_label: "CLI".into(),
                cli_id: None,
                resources: vec![Resource::Product(request.product_id.clone()), Resource::Npm],
            })
        }
        fn run(&self, _request: &TaskRequest) -> Result<String, String> {
            crate::installer::task_log("working");
            loop {
                if crate::installer::op_cancelled() {
                    return Err("已取消操作".into());
                }
                if let Ok(result) = self.gate.lock().unwrap().recv_timeout(Duration::from_millis(10)) {
                    return result;
                }
            }
        }
    }

    fn manager() -> (AgentTaskManager, mpsc::Sender<Result<String, String>>) {
        let (sender, receiver) = mpsc::channel();
        let runner = Arc::new(FakeRunner { gate: Mutex::new(receiver) });
        (AgentTaskManager::new(runner, Arc::new(|_| {})), sender)
    }

    fn request(id: &str) -> TaskRequest {
        TaskRequest { product_id: id.into(), surface: Surface::Cli, action: Action::Update }
    }

    fn wait_for(manager: &AgentTaskManager, id: &str, state: TaskState) -> AgentTask {
        for _ in 0..500 {
            if let Some(task) = manager.list().into_iter().find(|task| task.id == id && task.state == state) {
                return task;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("task {id} never reached {state:?}");
    }

    #[test]
    fn conflicting_tasks_queue_and_finish_in_order() {
        let (manager, gate) = manager();
        let first = manager.start(request("a")).unwrap();
        let second = manager.start(request("b")).unwrap();
        wait_for(&manager, &first.id, TaskState::Running);
        assert_eq!(manager.list().iter().find(|t| t.id == second.id).unwrap().state, TaskState::Queued);
        gate.send(Ok("done".into())).unwrap();
        wait_for(&manager, &first.id, TaskState::Succeeded);
        wait_for(&manager, &second.id, TaskState::Running);
        gate.send(Err("boom".into())).unwrap();
        let failed = wait_for(&manager, &second.id, TaskState::Failed);
        assert_eq!(failed.message.as_deref(), Some("boom"));
    }

    #[test]
    fn cancelling_one_task_leaves_the_queue_alone() {
        let (manager, gate) = manager();
        let first = manager.start(request("a")).unwrap();
        let second = manager.start(request("b")).unwrap();
        wait_for(&manager, &first.id, TaskState::Running);
        manager.cancel(&second.id).unwrap();
        wait_for(&manager, &second.id, TaskState::Cancelled);
        manager.cancel(&first.id).unwrap();
        wait_for(&manager, &first.id, TaskState::Cancelled);
        drop(gate);
    }

    #[test]
    fn duplicate_start_returns_the_open_task_and_logs_are_kept() {
        let (manager, gate) = manager();
        let first = manager.start(request("a")).unwrap();
        let again = manager.start(request("a")).unwrap();
        assert_eq!(first.id, again.id);
        wait_for(&manager, &first.id, TaskState::Running);
        gate.send(Ok("done".into())).unwrap();
        wait_for(&manager, &first.id, TaskState::Succeeded);
        assert_eq!(manager.log(&first.id).unwrap(), vec!["working".to_string()]);
        let retried = manager.retry(&first.id).unwrap();
        assert_ne!(retried.id, first.id);
        gate.send(Ok("done".into())).unwrap();
        wait_for(&manager, &retried.id, TaskState::Succeeded);
    }
}
```

- [ ] **Step 2: Run to verify failure.** Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib agents::tasks`. Expected: FAIL — types missing.

- [ ] **Step 3: Implement the manager** (in `tasks/mod.rs`, after `pub mod schedule; pub mod runner;` and the interface types above)

```rust
use schedule::{runnable, Resource};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const MAX_LOG_LINES: usize = 200;
const MAX_FINISHED: usize = 50;
const EMIT_INTERVAL: Duration = Duration::from_millis(250);

struct Record {
    task: AgentTask,
    request: TaskRequest,
    key: String,
    resources: Vec<Resource>,
    cancel: Arc<AtomicBool>,
    log: VecDeque<String>,
    last_emit: Option<Instant>,
}

struct Inner {
    next_id: u64,
    records: Vec<Record>,
}

pub struct AgentTaskManager {
    inner: Arc<Mutex<Inner>>,
    runner: Arc<dyn TaskRunner>,
    emit: Arc<dyn Fn(&AgentTask) + Send + Sync>,
}

fn now() -> String {
    chrono::Local::now().to_rfc3339()
}

fn is_open(state: TaskState) -> bool {
    matches!(state, TaskState::Queued | TaskState::Running)
}

impl AgentTaskManager {
    pub fn new(runner: Arc<dyn TaskRunner>, emit: Arc<dyn Fn(&AgentTask) + Send + Sync>) -> Self {
        Self { inner: Arc::new(Mutex::new(Inner { next_id: 1, records: Vec::new() })), runner, emit }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn start(&self, request: TaskRequest) -> Result<AgentTask, String> {
        let plan = self.runner.plan(&request)?;
        let task = {
            let mut inner = self.lock();
            if let Some(open) = inner.records.iter().find(|record| {
                record.key == plan.key && record.request.surface == request.surface && is_open(record.task.state)
            }) {
                return Ok(open.task.clone());
            }
            let id = format!("agent-task-{}", inner.next_id);
            inner.next_id += 1;
            let task = AgentTask {
                id,
                product_id: request.product_id.clone(),
                product_name: plan.product_name,
                surface: request.surface,
                surface_label: plan.surface_label,
                cli_id: plan.cli_id,
                action: request.action,
                state: TaskState::Queued,
                message: None,
                last_line: None,
                created_at: now(),
                started_at: None,
                finished_at: None,
            };
            inner.records.push(Record {
                task: task.clone(),
                request,
                key: plan.key,
                resources: plan.resources,
                cancel: Arc::new(AtomicBool::new(false)),
                log: VecDeque::new(),
                last_emit: None,
            });
            task
        };
        (self.emit)(&task);
        self.pump();
        Ok(task)
    }

    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let queued = {
            let mut inner = self.lock();
            let record = inner.records.iter_mut().find(|record| record.task.id == id).ok_or("任务不存在")?;
            record.cancel.store(true, Ordering::SeqCst);
            if record.task.state == TaskState::Queued {
                record.task.state = TaskState::Cancelled;
                record.task.finished_at = Some(now());
                Some(record.task.clone())
            } else {
                None
            }
        };
        if let Some(task) = queued {
            (self.emit)(&task);
        }
        Ok(())
    }

    pub fn retry(&self, id: &str) -> Result<AgentTask, String> {
        let request = {
            let inner = self.lock();
            let record = inner.records.iter().find(|record| record.task.id == id).ok_or("任务不存在")?;
            if is_open(record.task.state) {
                return Err("任务仍在执行".into());
            }
            record.request.clone()
        };
        self.start(request)
    }

    pub fn list(&self) -> Vec<AgentTask> {
        self.lock().records.iter().map(|record| record.task.clone()).collect()
    }

    pub fn log(&self, id: &str) -> Result<Vec<String>, String> {
        let inner = self.lock();
        let record = inner.records.iter().find(|record| record.task.id == id).ok_or("任务不存在")?;
        Ok(record.log.iter().cloned().collect())
    }

    pub fn running_count(&self) -> usize {
        self.lock().records.iter().filter(|record| is_open(record.task.state)).count()
    }

    pub fn cancel_all(&self) {
        let ids: Vec<_> = self
            .lock()
            .records
            .iter()
            .filter(|record| is_open(record.task.state))
            .map(|record| record.task.id.clone())
            .collect();
        for id in ids {
            let _ = self.cancel(&id);
        }
    }

    fn pump(&self) {
        let started: Vec<(AgentTask, TaskRequest, Arc<AtomicBool>)> = {
            let mut inner = self.lock();
            let running: Vec<Vec<Resource>> = inner
                .records
                .iter()
                .filter(|record| record.task.state == TaskState::Running)
                .map(|record| record.resources.clone())
                .collect();
            let queued_indexes: Vec<usize> = inner
                .records
                .iter()
                .enumerate()
                .filter(|(_, record)| record.task.state == TaskState::Queued)
                .map(|(index, _)| index)
                .collect();
            let queued: Vec<Vec<Resource>> =
                queued_indexes.iter().map(|&index| inner.records[index].resources.clone()).collect();
            runnable(&queued, &running)
                .into_iter()
                .map(|position| {
                    let record = &mut inner.records[queued_indexes[position]];
                    record.task.state = TaskState::Running;
                    record.task.started_at = Some(now());
                    (record.task.clone(), record.request.clone(), record.cancel.clone())
                })
                .collect()
        };
        for (task, request, cancel) in started {
            (self.emit)(&task);
            self.spawn(task.id, request, cancel);
        }
    }

    fn spawn(&self, id: String, request: TaskRequest, cancel: Arc<AtomicBool>) {
        let manager = AgentTaskManager {
            inner: self.inner.clone(),
            runner: self.runner.clone(),
            emit: self.emit.clone(),
        };
        std::thread::spawn(move || {
            let log_manager = AgentTaskManager {
                inner: manager.inner.clone(),
                runner: manager.runner.clone(),
                emit: manager.emit.clone(),
            };
            let log_id = id.clone();
            let context = crate::installer::TaskContext {
                cancel: cancel.clone(),
                log: Arc::new(move |line: &str| log_manager.append_log(&log_id, line)),
            };
            let runner = manager.runner.clone();
            let outcome = crate::installer::with_task_context(context, || {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runner.run(&request)))
            });
            let (state, message) = match outcome {
                _ if cancel.load(Ordering::SeqCst) => (TaskState::Cancelled, Some("已取消".to_string())),
                Ok(Ok(message)) => (TaskState::Succeeded, Some(message)),
                Ok(Err(error)) => (TaskState::Failed, Some(error)),
                Err(_) => (TaskState::Failed, Some("内部错误".to_string())),
            };
            manager.finish(&id, state, message);
            manager.pump();
        });
    }

    fn append_log(&self, id: &str, line: &str) {
        let emit = {
            let mut inner = self.lock();
            let Some(record) = inner.records.iter_mut().find(|record| record.task.id == id) else {
                return;
            };
            record.log.push_back(line.to_string());
            while record.log.len() > MAX_LOG_LINES {
                record.log.pop_front();
            }
            record.task.last_line = Some(line.to_string());
            let due = record.last_emit.map_or(true, |at| at.elapsed() >= EMIT_INTERVAL);
            if due {
                record.last_emit = Some(Instant::now());
            }
            due.then(|| record.task.clone())
        };
        if let Some(task) = emit {
            (self.emit)(&task);
        }
    }

    fn finish(&self, id: &str, state: TaskState, message: Option<String>) {
        let task = {
            let mut inner = self.lock();
            let Some(record) = inner.records.iter_mut().find(|record| record.task.id == id) else {
                return;
            };
            record.task.state = state;
            record.task.message = message;
            record.task.finished_at = Some(now());
            let task = record.task.clone();
            let finished = inner.records.iter().filter(|record| !is_open(record.task.state)).count();
            if finished > MAX_FINISHED {
                let mut drop = finished - MAX_FINISHED;
                inner.records.retain(|record| {
                    if drop > 0 && !is_open(record.task.state) {
                        drop -= 1;
                        false
                    } else {
                        true
                    }
                });
            }
            task
        };
        (self.emit)(&task);
    }
}
```

- [ ] **Step 4: Run the manager tests.** Expected: the three tests pass.

- [ ] **Step 5: Write the failing post-check tests** (in `tasks/runner.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn surface(health: &str, version: Option<&str>, update_available: bool) -> VibeSurface {
        let mut surface = crate::agents::test_surface();
        surface.health = health.into();
        surface.version = version.map(Into::into);
        surface.update_available = update_available;
        surface
    }

    #[test]
    fn update_must_leave_a_healthy_changed_install() {
        assert!(verify_outcome(Action::Update, Some("1.0"), &surface("healthy", Some("1.1"), false)).is_ok());
        assert!(verify_outcome(Action::Update, Some("1.0"), &surface("healthy", Some("1.0"), true)).is_err());
        assert!(verify_outcome(Action::Update, Some("1.0"), &surface("healthy", Some("1.0"), false)).is_ok());
        assert!(verify_outcome(Action::Update, Some("1.0"), &surface("broken", None, false)).is_err());
    }

    #[test]
    fn install_repair_and_uninstall_rules() {
        assert!(verify_outcome(Action::Install, None, &surface("healthy", Some("1.0"), false)).is_ok());
        assert!(verify_outcome(Action::Install, None, &surface("missing", None, false)).is_err());
        assert!(verify_outcome(Action::Repair, None, &surface("broken", None, false)).is_err());
        assert!(verify_outcome(Action::Uninstall, None, &surface("missing", None, false)).is_ok());
        assert!(verify_outcome(Action::Uninstall, None, &surface("healthy", Some("1.0"), false)).is_err());
    }
}
```

Add to `agents/mod.rs`:

```rust
#[cfg(test)]
pub(crate) fn test_surface() -> VibeSurface {
    pending_surface("Test", "CLI", "", "test", "", "", true)
}
```

- [ ] **Step 6: Run to verify failure.** Expected: FAIL — `verify_outcome` missing.

- [ ] **Step 7: Implement `runner.rs`**

```rust
use super::schedule::Resource;
use super::{Action, Surface, TaskPlan, TaskRequest, TaskRunner};
use crate::agents::registry::spec_by_id;
use crate::agents::VibeSurface;

pub fn verify_outcome(action: Action, before_version: Option<&str>, after: &VibeSurface) -> Result<(), String> {
    let reason = || after.broken_reason.clone().unwrap_or_else(|| "未检测到可用入口".into());
    match action {
        Action::Install | Action::Repair if after.health == "healthy" => Ok(()),
        Action::Install => Err(format!("安装后校验未通过：{}", reason())),
        Action::Repair => Err(format!("修复后校验未通过：{}", reason())),
        Action::Update if after.health != "healthy" => Err(format!("更新后校验未通过：{}", reason())),
        Action::Update if before_version.is_some()
            && after.version.as_deref() == before_version
            && after.update_available =>
        {
            Err("更新后校验未通过：版本未变化".into())
        }
        Action::Update => Ok(()),
        Action::Uninstall if after.health == "missing" => Ok(()),
        Action::Uninstall => Err("卸载后仍检测到入口".into()),
    }
}

pub struct ProductionRunner;

fn surface_of(tool: &crate::agents::VibeTool, surface: Surface) -> &VibeSurface {
    match surface {
        Surface::Cli => &tool.cli,
        Surface::Desktop => &tool.desktop,
    }
}

impl TaskRunner for ProductionRunner {
    fn plan(&self, request: &TaskRequest) -> Result<TaskPlan, String> {
        let spec = spec_by_id(&request.product_id).ok_or("未知的智能体")?;
        let cached = crate::agents::cached_tool(&request.product_id);
        let method = cached
            .as_ref()
            .and_then(|tool| surface_of(tool, request.surface).install_method.clone());
        let mut resources = Vec::new();
        let (key, label) = match request.surface {
            Surface::Cli => {
                let key = spec.cli_id.ok_or("该智能体没有 CLI")?.to_string();
                let uses_npm = spec.cli.npm_package.is_some()
                    || matches!(method.as_deref(), Some("npm") | Some("conda-npm"));
                if uses_npm {
                    resources.push(Resource::Npm);
                }
                if spec.cli.winget_id.is_some() || method.as_deref() == Some("winget") {
                    resources.push(Resource::Installer);
                }
                (key, spec.cli.name.to_string())
            }
            Surface::Desktop => {
                if !spec.desktop_available {
                    return Err("该智能体没有桌面端".into());
                }
                resources.push(Resource::Installer);
                (spec.id.to_string(), spec.desktop.name.to_string())
            }
        };
        if request.action != Action::Uninstall {
            resources.push(Resource::Download);
        }
        resources.insert(0, Resource::Product(key.clone()));
        Ok(TaskPlan {
            key,
            product_name: spec.name.to_string(),
            surface_label: label,
            cli_id: spec.cli_id.map(Into::into),
            resources,
        })
    }

    fn run(&self, request: &TaskRequest) -> Result<String, String> {
        let target = match request.surface {
            Surface::Cli => "cli",
            Surface::Desktop => "desktop",
        };
        let action = match request.action {
            Action::Install => "install",
            Action::Update => "update",
            Action::Uninstall => "uninstall",
            Action::Repair => "repair",
        };
        let before = crate::agents::fresh_tool(&request.product_id, false)
            .and_then(|tool| surface_of(&tool, request.surface).version.clone());
        let result = crate::agents::run_tool_action(&request.product_id, target, action, None);
        crate::agents::invalidate_vibe_scan_cache();
        let after = crate::agents::fresh_tool(&request.product_id, true).ok_or("无法重新检测该智能体")?;
        let verified = verify_outcome(request.action, before.as_deref(), surface_of(&after, request.surface));
        match (result, verified) {
            (Ok(message), Ok(())) => Ok(message),
            (Ok(_), Err(reason)) => Err(reason),
            (Err(error), Ok(())) if request.action != Action::Uninstall => {
                crate::installer::task_log(&format!("安装器返回错误：{error}"));
                Ok("已完成（安装器返回错误，但复检通过）".into())
            }
            (Err(error), _) => Err(error),
        }
    }
}
```

Add to `agents/mod.rs` (and make `run_tool_action`, `invalidate_vibe_scan_cache` `pub(crate)`):

```rust
pub(crate) fn cached_tool(id: &str) -> Option<VibeTool> {
    let cache = VIBE_SCAN_CACHE.get()?;
    let guard = cache.lock().ok()?;
    guard.as_ref()?.1.iter().find(|tool| tool.id == id).cloned()
}

pub(crate) fn fresh_tool(id: &str, check_latest: bool) -> Option<VibeTool> {
    let tool = scan_vibe_tool(id, check_latest)?;
    cache_vibe_tool(&tool);
    Some(tool)
}
```

- [ ] **Step 8: Add the repair action**

In `agents/mod.rs::run_tool_action` add the arm `("cli", "repair") => install::repair_cli_tool(&spec, &window),`. In `install/mod.rs`:

```rust
/// Removes a broken effective CLI entry so the next healthy install on PATH takes over.
pub(crate) fn repair_cli_tool(spec: &ToolSpec, window: &Option<tauri::Window>) -> Result<String, String> {
    let tool = super::fresh_tool(spec.id, false).ok_or("无法检测该智能体")?;
    if !tool.cli.can_repair {
        return Err("当前没有可自动修复的损坏入口".into());
    }
    emit_progress(window, format!("正在移除损坏的入口：{}", tool.cli.path.clone().unwrap_or_default()));
    uninstall_cli_tool(spec, window)?;
    Ok(format!("{} 已修复，当前使用健康的安装", spec.cli.name))
}
```

- [ ] **Step 9: Commands, state and events**

Append to `agents/commands.rs`:

```rust
use super::tasks::{AgentTask, AgentTaskManager, TaskRequest};

#[tauri::command]
pub fn agent_task_start(request: TaskRequest, manager: tauri::State<'_, AgentTaskManager>) -> Result<AgentTask, String> {
    manager.start(request)
}

#[tauri::command]
pub fn agent_task_cancel(id: String, manager: tauri::State<'_, AgentTaskManager>) -> Result<(), String> {
    manager.cancel(&id)
}

#[tauri::command]
pub fn agent_task_retry(id: String, manager: tauri::State<'_, AgentTaskManager>) -> Result<AgentTask, String> {
    manager.retry(&id)
}

#[tauri::command]
pub fn agent_tasks(manager: tauri::State<'_, AgentTaskManager>) -> Vec<AgentTask> {
    manager.list()
}

#[tauri::command]
pub fn agent_task_log(id: String, manager: tauri::State<'_, AgentTaskManager>) -> Result<Vec<String>, String> {
    manager.log(&id)
}
```

`agent_task_start` calls `runner.plan`, which only reads the registry and the scan cache, so it is safe as a sync command.

In `lib.rs` `.setup(|app| { ... })` add, before the tray is built:

```rust
{
    use tauri::{Emitter, Manager};
    let handle = app.handle().clone();
    app.manage(agents::tasks::AgentTaskManager::new(
        std::sync::Arc::new(agents::tasks::runner::ProductionRunner),
        std::sync::Arc::new(move |task: &agents::tasks::AgentTask| {
            let _ = handle.emit("agent-task", task);
        }),
    ));
}
```

Register the five commands in `invoke_handler`. Replace the tray `"quit" => app.exit(0),` arm with:

```rust
"quit" => {
    use tauri::Manager;
    let running = app.state::<agents::tasks::AgentTaskManager>().running_count();
    if running == 0 {
        app.exit(0);
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
        let confirmed = app
            .dialog()
            .message(format!("还有 {running} 个智能体任务正在执行，退出会中断这些任务。确定退出吗？"))
            .buttons(MessageDialogButtons::OkCancelCustom("退出".into(), "取消".into()))
            .blocking_show();
        if confirmed {
            app.state::<agents::tasks::AgentTaskManager>().cancel_all();
            app.exit(0);
        }
    });
}
```

If the close-behavior setting also exits the app from the window close handler in `lib.rs`, apply the same check there.

- [ ] **Step 10: Run tests and the full Rust check.** Expected: pass.

- [ ] **Step 11: Commit** — `feat: run agent actions as scheduled background tasks`.

---

### Task 10: One-click update plan

**Files:**
- Create: `src-tauri/src/agents/tasks/plan.rs`
- Modify: `src-tauri/src/agents/tasks/mod.rs` (`pub mod plan;`), `src-tauri/src/agents/commands.rs`, `src-tauri/src/lib.rs`
- Test: `src-tauri/src/agents/tasks/plan.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub product_id: String,
    pub product_name: String,
    pub surface: Surface,
    pub surface_label: String,
    pub current: Option<String>,
    pub latest: Option<String>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePlan { pub auto: Vec<PlanItem>, pub manual: Vec<PlanItem> }

pub fn build_update_plan(tools: &[VibeTool]) -> UpdatePlan;
```

  Commands: `agent_update_plan() -> UpdatePlan` (async, uses the cached scan and runs a fresh scan when the cache is empty), `agent_update_all() -> Vec<AgentTask>` (builds the plan and starts one task per `auto` item).

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn tool(id: &str, cli_id: Option<&str>, cli: VibeSurface, desktop: VibeSurface) -> VibeTool {
        let mut tool = crate::agents::test_tool(id);
        tool.cli_id = cli_id.map(Into::into);
        tool.cli = cli;
        tool.desktop = desktop;
        tool
    }

    fn updatable(can_update: bool, health: &str) -> VibeSurface {
        let mut surface = crate::agents::test_surface();
        surface.update_available = true;
        surface.can_update = can_update;
        surface.health = health.into();
        surface.version = Some("1.0".into());
        surface.latest = Some("2.0".into());
        surface
    }

    #[test]
    fn splits_auto_and_manual_and_dedupes_shared_cli() {
        let idle = crate::agents::test_surface();
        let tools = vec![
            tool("workbuddy-cn", Some("codebuddy"), updatable(true, "healthy"), updatable(false, "healthy")),
            tool("workbuddy-global", Some("codebuddy"), updatable(true, "healthy"), idle.clone()),
            tool("claude", Some("claude"), updatable(true, "broken"), idle),
        ];
        let plan = build_update_plan(&tools);
        assert_eq!(plan.auto.len(), 1);
        assert_eq!(plan.auto[0].product_id, "workbuddy-cn");
        assert_eq!(plan.manual.len(), 2);
        assert!(plan.manual.iter().any(|item| item.surface == Surface::Desktop && item.reason.as_deref() == Some("需要在官方下载页手动更新")));
        assert!(plan.manual.iter().any(|item| item.product_id == "claude" && item.reason.as_deref().unwrap().contains("损坏")));
    }
}
```

Add to `agents/mod.rs`:

```rust
#[cfg(test)]
pub(crate) fn test_tool(id: &str) -> VibeTool {
    VibeTool {
        id: id.into(),
        family_id: id.into(),
        edition: "global".into(),
        edition_label: String::new(),
        sort_order: 0,
        name: id.into(),
        description: String::new(),
        docs_url: String::new(),
        icon: String::new(),
        cli_id: None,
        cli_note: None,
        cli: test_surface(),
        desktop: test_surface(),
    }
}
```

- [ ] **Step 2: Run to verify failure.** Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
use super::Surface;
use crate::agents::{VibeSurface, VibeTool};
use serde::Serialize;

fn item(tool: &VibeTool, surface: Surface, state: &VibeSurface, reason: Option<&str>) -> PlanItem {
    PlanItem {
        product_id: tool.id.clone(),
        product_name: tool.name.clone(),
        surface,
        surface_label: state.label.clone(),
        current: state.version.clone(),
        latest: state.latest.clone(),
        reason: reason.map(Into::into),
    }
}

pub fn build_update_plan(tools: &[VibeTool]) -> UpdatePlan {
    let mut plan = UpdatePlan::default();
    let mut seen_cli = std::collections::HashSet::new();
    for tool in tools {
        for (surface, state) in [(Surface::Cli, &tool.cli), (Surface::Desktop, &tool.desktop)] {
            if !state.update_available {
                continue;
            }
            if surface == Surface::Cli {
                if let Some(cli_id) = &tool.cli_id {
                    if !seen_cli.insert(cli_id.clone()) {
                        continue;
                    }
                }
            }
            if state.health == "broken" {
                plan.manual.push(item(tool, surface, state, Some("安装已损坏，请先修复")));
            } else if !state.can_update {
                plan.manual.push(item(tool, surface, state, Some("需要在官方下载页手动更新")));
            } else {
                plan.auto.push(item(tool, surface, state, None));
            }
        }
    }
    plan
}
```

Commands in `commands.rs`:

```rust
#[tauri::command]
pub async fn agent_update_plan() -> super::tasks::plan::UpdatePlan {
    tauri::async_runtime::spawn_blocking(|| super::tasks::plan::build_update_plan(&super::scan_vibe_tools_cached()))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn agent_update_all(app: tauri::AppHandle) -> Result<Vec<AgentTask>, String> {
    use tauri::Manager;
    let plan = agent_update_plan().await;
    let manager = app.state::<AgentTaskManager>();
    plan.auto
        .into_iter()
        .map(|item| {
            manager.start(TaskRequest {
                product_id: item.product_id,
                surface: item.surface,
                action: super::tasks::Action::Update,
            })
        })
        .collect()
}
```

Make `scan_vibe_tools_cached` `pub(crate)`. Register both commands in `lib.rs`.

- [ ] **Step 4: Run tests and the full Rust check.** Expected: pass.

- [ ] **Step 5: Commit** — `feat: add the one-click agent update plan`.

---

### Task 11: Remove managed work sessions

**Files:**
- Delete: `src-tauri/src/work_session.rs`, `src/features/agent-workspace/` (all files)
- Modify: `src-tauri/src/lib.rs` (remove `mod work_session;` and its nine command registrations, remove `agents::commands::vibe_agent_activity` and `vibe_agent_environment` registrations), `src-tauri/src/agents/commands.rs` (delete those two commands), `src-tauri/src/agents/activity.rs` (delete `scan_agent_environment`, `AgentEnvironmentSnapshot` usage), `src-tauri/src/agents/mod.rs` (delete `ManagedCli`, `ManagedDesktop`, `managed_agent`, `managed_cli`, `managed_desktop`, `AgentEnvironmentSnapshot`), `src-tauri/src/agents/activity.rs` (delete `desktop_agent_processes`, `is_probable_desktop_process` if no caller remains), `src/pages/AgentSpace.tsx`, `src/features/conversations/ConversationManager.tsx`, `src/i18n.ts` (delete `agentSpace.*` keys)

- [ ] **Step 1: Delete the backend pieces and fix compilation**

```bash
git rm src-tauri/src/work_session.rs
```

Remove the listed items. Run `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` and delete anything clippy now reports as dead code that belonged only to work sessions.

- [ ] **Step 2: Delete the frontend pieces**

```bash
git rm -r src/features/agent-workspace
```

- `ConversationManager.tsx`: remove the `advanced` prop, the `advancedOpen` state, and the "高级：环境约束与空间跟踪记录" toggle (lines 17, 40, 196-197). Signature becomes `export function ConversationManager({ onCleanup }: { onCleanup: () => void })`.
- `AgentSpace.tsx` becomes:

```tsx
import { ConversationManager } from "../features/conversations/ConversationManager";
import type { Page } from "../pageState";

export default function AgentSpace({ goto }: { goto: (page: Page) => void }) {
  return <ConversationManager onCleanup={() => goto("cleanup")} />;
}
```

- Remove every `agentSpace.*` key from both locale blocks in `src/i18n.ts`, and remove any `en.generated.ts` entries whose Chinese key no longer appears in `src/` or `src-tauri/src` (run `npm run check:i18n`; it reports stale keys if the script supports that, otherwise search each removed string with `rg`).

- [ ] **Step 3: Run the full Rust and frontend checks.** Expected: pass. `rg -n "work_session|agent-workspace|ManagedWorkSession" src src-tauri/src` returns nothing.

- [ ] **Step 4: Commit** — `refactor: remove managed work sessions`.

---

### Task 12: Sectioned sidebar, renamed pages and the catalog store

**Files:**
- Create: `src/navigation.ts`, `src/navigation.test.ts`, `src/features/agents/catalogStore.ts`
- Rename: `src/pages/Vibe.tsx` → `src/pages/Agents.tsx`, `src/pages/Vibe.test.tsx` → `src/pages/Agents.test.tsx`, `src/pages/AgentSpace.tsx` → `src/pages/AgentData.tsx`
- Modify: `src/pageState.ts`, `src/pageState.test.ts`, `src/App.tsx`, `src/notifications.tsx`, `src/i18n.ts`, `src/en.generated.ts`, `src/styles.css`

**Interfaces:**
- Produces:
  - `PAGE_IDS` with `"agents"` and `"agent-data"` in place of `"vibe"` and `"agent-space"`; `normalizePage` maps the legacy ids.
  - `navigation.ts`: `export type NavItem = { id: Page; icon: string; labelKey: MessageKey }; export type NavSection = { labelKey: MessageKey | null; items: NavItem[] }; export const NAV_SECTIONS: NavSection[]; export const NAV_FOOT: NavItem[]; export const ALL_NAV_ITEMS: NavItem[];`
  - `catalogStore.ts` exports the types `VibeSurface`, `VibeTool`, `InstallInfo` and the functions `vibeSnapshot`, `subscribeVibe`, `runVibeCheck`, `refreshOneTool`, `refreshTools(ids: string[])`, `loadCatalog()`, `surfaceDetected` (moved from `Vibe.tsx`).

- [ ] **Step 1: Write the failing tests**

`src/pageState.test.ts` — replace the `agent-space` case with:

```ts
it("maps legacy agent page ids to the new pages", () => {
  expect(readLastPage(memoryStorage("vibe"))).toBe("agents");
  expect(readLastPage(memoryStorage("agent-space"))).toBe("agent-data");
  expect(readLastPage(memoryStorage("agent-data"))).toBe("agent-data");
});
```

`src/navigation.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { ALL_NAV_ITEMS, NAV_SECTIONS } from "./navigation";
import { PAGE_IDS } from "./pageState";
import { translateText } from "./i18n";
import { MESSAGES_ZH } from "./i18n";

describe("sidebar navigation", () => {
  it("lists every page exactly once", () => {
    expect([...ALL_NAV_ITEMS.map((item) => item.id)].sort()).toEqual([...PAGE_IDS].sort());
  });

  it("uses five-character section titles and four-character Chinese menu items", () => {
    const productPages = new Set(["git", "python", "php", "node", "java", "maven", "gradle", "go", "rust"]);
    expect(NAV_SECTIONS[1].labelKey).toBe("nav.section.agents");
    for (const section of NAV_SECTIONS) {
      if (section.labelKey) expect([...MESSAGES_ZH[section.labelKey]]).toHaveLength(5);
      for (const item of section.items) {
        if (!productPages.has(item.id)) expect([...MESSAGES_ZH[item.labelKey]], item.id).toHaveLength(4);
      }
    }
    void translateText;
  });
});
```

If `src/i18n.ts` does not export the Chinese message table, export it as `MESSAGES_ZH` (the object that currently holds `"nav.overview": "编程生态体检"`).

- [ ] **Step 2: Run to verify failure.** Run: `npx vitest run src/pageState.test.ts src/navigation.test.ts`. Expected: FAIL.

- [ ] **Step 3: Implement page ids and navigation**

`src/pageState.ts`: replace `"vibe"`, `"agent-space"` in `PAGE_IDS` with `"agents"`, `"agent-data"`, and:

```ts
const LEGACY_PAGE_IDS: Record<string, Page> = { vibe: "agents", "agent-space": "agent-data" };

export function normalizePage(value: unknown): Page {
  if (typeof value !== "string") return DEFAULT_PAGE;
  const mapped = LEGACY_PAGE_IDS[value] ?? value;
  return PAGE_ID_SET.has(mapped) ? mapped as Page : DEFAULT_PAGE;
}
```

`src/navigation.ts`:

```ts
import type { MessageKey } from "./i18n";
import type { Page } from "./pageState";

export type NavItem = { id: Page; icon: string; labelKey: MessageKey };
export type NavSection = { labelKey: MessageKey | null; items: NavItem[] };

export const NAV_SECTIONS: NavSection[] = [
  { labelKey: null, items: [{ id: "overview", icon: "ti-layout-dashboard", labelKey: "nav.overview" }] },
  {
    labelKey: "nav.section.agents",
    items: [
      { id: "agents", icon: "ti-sparkles", labelKey: "nav.agents" },
      { id: "agent-data", icon: "ti-database", labelKey: "nav.agentData" },
    ],
  },
  {
    labelKey: "nav.section.devEnv",
    items: [
      { id: "git", icon: "ti-brand-git", labelKey: "nav.git" },
      { id: "python", icon: "ti-brand-python", labelKey: "nav.python" },
      { id: "php", icon: "ti-brand-php", labelKey: "nav.php" },
      { id: "node", icon: "ti-brand-nodejs", labelKey: "nav.node" },
      { id: "java", icon: "ti-coffee", labelKey: "nav.java" },
      { id: "maven", icon: "ti-feather", labelKey: "nav.maven" },
      { id: "gradle", icon: "ti-box", labelKey: "nav.gradle" },
      { id: "go", icon: "ti-brand-golang", labelKey: "nav.go" },
      { id: "rust", icon: "ti-brand-rust", labelKey: "nav.rust" },
    ],
  },
  {
    labelKey: "nav.section.system",
    items: [
      { id: "proxy", icon: "ti-world-bolt", labelKey: "nav.proxy" },
      { id: "cleanup", icon: "ti-eraser", labelKey: "nav.cleanup" },
    ],
  },
];

export const NAV_FOOT: NavItem[] = [
  { id: "history", icon: "ti-history", labelKey: "nav.history" },
  { id: "settings", icon: "ti-settings", labelKey: "nav.settings" },
];

export const ALL_NAV_ITEMS: NavItem[] = [...NAV_SECTIONS.flatMap((section) => section.items), ...NAV_FOOT];
```

`src/i18n.ts` messages (Chinese block / English block):

| key | zh | en |
| --- | --- | --- |
| `nav.overview` | 环境体检 | Checkup |
| `nav.section.agents` | 智能体管理 | Agents |
| `nav.agents` | 安装更新 | Install & Update |
| `nav.agentData` | 会话数据 | Sessions & Data |
| `nav.section.devEnv` | 开发工具链 | Dev Toolchain |
| `nav.node` | Node.js | Node.js |
| `nav.section.system` | 网络与存储 | Network & Storage |
| `nav.proxy` | 终端代理 | Proxy |
| `nav.cleanup` | 磁盘清理 | Disk Cleanup |
| `nav.history` | 配置备份 | Backups |
| `nav.settings` | 偏好设置 | Settings |

Delete `nav.vibe` and `nav.agentSpace`.

`src/App.tsx`: remove `NAV_TOP`, `NAV_TOOLS`, `NAV_FOOT`, `ALL`, `NavItem` and import them from `./navigation` (`ALL` → `ALL_NAV_ITEMS`). Render the nav as:

```tsx
<nav>
  {NAV_SECTIONS.map((section, index) => (
    <div className="navsection" key={section.labelKey ?? `top-${index}`}>
      {section.labelKey && <div className="navlabel">{t(section.labelKey)}</div>}
      {section.items.map((n) => <NavBtn key={n.id} item={n} page={page} set={setPage} />)}
    </div>
  ))}
</nav>
```

Page switch: `page === "agents" ? <Agents key={configEpoch} />` and `page === "agent-data" ? <AgentData key={configEpoch} goto={setPage} />`, with imports updated to the renamed files.

`src/styles.css`: add after `.a .navlabel{...}`:

```css
.a nav .navsection + .navsection{ margin-top:6px; }
.a nav .navsection:first-child .ni{ margin-bottom:2px; }
```

- [ ] **Step 4: Extract the catalog store and rename pages**

```bash
git mv src/pages/Vibe.tsx src/pages/Agents.tsx
git mv src/pages/Vibe.test.tsx src/pages/Agents.test.tsx
git mv src/pages/AgentSpace.tsx src/pages/AgentData.tsx
```

Move from `Agents.tsx` into `src/features/agents/catalogStore.ts`: the `VibeSurface` and `VibeTool` types (add the new backend fields `health: "healthy" | "broken" | "missing"; broken_reason?: string | null; other_installs: InstallInfo[]; can_repair: boolean;` on `VibeSurface`, `status` union gains `"broken"`, and `icon: string; cli_id?: string | null; cli_note?: string | null;` on `VibeTool`), `InstallInfo = { path: string; healthy: boolean; version?: string | null; reason?: string | null }`, `VibeCache`, `VIBE_CACHE_KEY` (bump to `"stacker.vibe.status.v3"`), `restoreVibeCache`, `publishVibe`, `vibeSnapshot`, `subscribeVibe`, `runVibeCheck`, `refreshOneTool`, `surfaceDetected`, and the `vibe_catalog` merge effect body as `export async function loadCatalog()`. Add:

```ts
export async function refreshTools(ids: string[]) {
  for (const id of new Set(ids)) await refreshOneTool(id);
}
```

`surfaceDetected` becomes `surface.health === "healthy" || surface.health === "broken" || surface.installed || !!surface.path` so broken installs still show their row actions.

Update imports in `Agents.tsx`, `Agents.test.tsx` (add `health: "missing", other_installs: [], can_repair: false` to its fixture), `AgentData.tsx` (it no longer needs the catalog), and `src/notifications.tsx` (`page: "vibe"` → `page: "agents"` everywhere, type import from `./features/agents/catalogStore`).

- [ ] **Step 5: Run the frontend checks.** Expected: pass; the navigation test proves every page appears once, section titles are five characters and Chinese menu items are four.

- [ ] **Step 6: Commit** — `feat: sectioned sidebar and renamed agent pages`.

---

### Task 13: Frontend task store, task center and toasts

**Files:**
- Create: `src/features/agent-tasks/taskStore.ts`, `src/features/agent-tasks/taskStore.test.ts`, `src/features/agent-tasks/TaskCenter.tsx`, `src/features/agent-tasks/useTaskToasts.ts`, `src/features/agent-tasks/agentTasks.css`
- Modify: `src/App.tsx` (header button, toast hook, CSS import), `src/en.generated.ts`

**Interfaces:**
- Consumes: backend `agent-task` event and commands from Tasks 9-10; `refreshTools` from Task 12.
- Produces (`taskStore.ts`):

```ts
export type AgentTaskState = "queued" | "running" | "succeeded" | "failed" | "cancelled";
export type AgentTaskAction = "install" | "update" | "uninstall" | "repair";
export type AgentTask = {
  id: string; productId: string; productName: string; surface: "cli" | "desktop"; surfaceLabel: string;
  cliId: string | null; action: AgentTaskAction; state: AgentTaskState; message: string | null;
  lastLine: string | null; createdAt: string; startedAt: string | null; finishedAt: string | null;
};
export type TaskMap = Record<string, AgentTask>;
export function isOpenTask(task: AgentTask): boolean;
export function applyTaskEvent(tasks: TaskMap, next: AgentTask): { tasks: TaskMap; finished: AgentTask | null };
export function openTaskFor(tasks: TaskMap, productId: string, cliId: string | null | undefined, surface: "cli" | "desktop"): AgentTask | null;
export function taskSnapshot(): TaskMap;
export function subscribeTasks(fn: (tasks: TaskMap) => void): () => void;
export function initAgentTasks(onFinished: (task: AgentTask) => void): Promise<() => void>;
export function startAgentTask(productId: string, surface: "cli" | "desktop", action: AgentTaskAction): Promise<AgentTask>;
export function cancelAgentTask(id: string): Promise<void>;
export function retryAgentTask(id: string): Promise<AgentTask>;
export function agentTaskLog(id: string): Promise<string[]>;
```

- [ ] **Step 1: Write the failing tests** (`taskStore.test.ts`)

```ts
import { describe, expect, it } from "vitest";
import { applyTaskEvent, openTaskFor, type AgentTask, type TaskMap } from "./taskStore";

function task(overrides: Partial<AgentTask>): AgentTask {
  return {
    id: "t1", productId: "workbuddy-cn", productName: "WorkBuddy 中国版", surface: "cli", surfaceLabel: "CodeBuddy CLI",
    cliId: "codebuddy", action: "update", state: "queued", message: null, lastLine: null,
    createdAt: "2026-09-18T00:00:00Z", startedAt: null, finishedAt: null, ...overrides,
  };
}

describe("agent task store", () => {
  it("reports a finished task exactly once", () => {
    let tasks: TaskMap = {};
    let result = applyTaskEvent(tasks, task({ state: "running" }));
    expect(result.finished).toBeNull();
    tasks = result.tasks;
    result = applyTaskEvent(tasks, task({ state: "succeeded" }));
    expect(result.finished?.id).toBe("t1");
    result = applyTaskEvent(result.tasks, task({ state: "succeeded" }));
    expect(result.finished).toBeNull();
  });

  it("finds the open task of a shared CLI from either product card", () => {
    const tasks: TaskMap = { t1: task({ state: "running" }) };
    expect(openTaskFor(tasks, "workbuddy-global", "codebuddy", "cli")?.id).toBe("t1");
    expect(openTaskFor(tasks, "workbuddy-global", "codebuddy", "desktop")).toBeNull();
    expect(openTaskFor({ t1: task({ state: "failed" }) }, "workbuddy-cn", "codebuddy", "cli")).toBeNull();
  });
});
```

- [ ] **Step 2: Run to verify failure.** Run: `npx vitest run src/features/agent-tasks`. Expected: FAIL.

- [ ] **Step 3: Implement `taskStore.ts`**

```ts
import { listen } from "@tauri-apps/api/event";
import { invoke } from "../../invoke";

// (types from the Interfaces block)

export function isOpenTask(task: AgentTask) {
  return task.state === "queued" || task.state === "running";
}

export function applyTaskEvent(tasks: TaskMap, next: AgentTask) {
  const previous = tasks[next.id];
  const finished = !isOpenTask(next) && (!previous || isOpenTask(previous)) ? next : null;
  return { tasks: { ...tasks, [next.id]: next }, finished };
}

export function openTaskFor(tasks: TaskMap, productId: string, cliId: string | null | undefined, surface: "cli" | "desktop") {
  return Object.values(tasks).find((task) => isOpenTask(task) && task.surface === surface
    && (surface === "cli" && cliId ? task.cliId === cliId : task.productId === productId)) ?? null;
}

let taskMap: TaskMap = {};
const listeners = new Set<(tasks: TaskMap) => void>();

function publish(next: TaskMap) {
  taskMap = next;
  listeners.forEach((fn) => fn(taskMap));
}

export function taskSnapshot() {
  return taskMap;
}

export function subscribeTasks(fn: (tasks: TaskMap) => void) {
  listeners.add(fn);
  return () => { listeners.delete(fn); };
}

export async function initAgentTasks(onFinished: (task: AgentTask) => void) {
  const unlisten = await listen<AgentTask>("agent-task", (event) => {
    const result = applyTaskEvent(taskMap, event.payload);
    publish(result.tasks);
    if (result.finished) onFinished(result.finished);
  });
  const existing = await invoke<AgentTask[]>("agent_tasks");
  publish(Object.fromEntries(existing.map((task) => [task.id, task])));
  return unlisten;
}

export function startAgentTask(productId: string, surface: "cli" | "desktop", action: AgentTaskAction) {
  return invoke<AgentTask>("agent_task_start", { request: { productId, surface, action } });
}

export function cancelAgentTask(id: string) {
  return invoke<void>("agent_task_cancel", { id });
}

export function retryAgentTask(id: string) {
  return invoke<AgentTask>("agent_task_retry", { id });
}

export function agentTaskLog(id: string) {
  return invoke<string[]>("agent_task_log", { id });
}
```

- [ ] **Step 4: Toasts** (`useTaskToasts.ts`)

```ts
import { useEffect } from "react";
import { useToast } from "../../ui";
import { translateText } from "../../i18n";
import { refreshTools, vibeSnapshot } from "../agents/catalogStore";
import { initAgentTasks, type AgentTask } from "./taskStore";

const ACTION_TEXT: Record<AgentTask["action"], string> = { install: "安装", update: "更新", uninstall: "卸载", repair: "修复" };

export function useTaskToasts() {
  const toast = useToast();
  useEffect(() => {
    let dispose: (() => void) | undefined;
    let active = true;
    void initAgentTasks((task) => {
      const action = translateText(ACTION_TEXT[task.action]);
      if (task.state === "succeeded") toast(`${task.surfaceLabel} ${action}${translateText("完成")}`, "ok");
      else if (task.state === "failed") toast(`${task.surfaceLabel} ${action}${translateText("失败")}：${task.message ?? ""}（${translateText("可在任务面板查看日志")}）`, "err");
      else toast(`${translateText("已取消")} ${task.surfaceLabel} ${action}`, "info");
      const siblings = task.cliId
        ? vibeSnapshot().tools.filter((tool) => tool.cli_id === task.cliId).map((tool) => tool.id)
        : [];
      void refreshTools([task.productId, ...siblings]).catch(() => undefined);
    }).then((unlisten) => {
      if (active) dispose = unlisten;
      else unlisten();
    });
    return () => { active = false; dispose?.(); };
  }, [toast]);
}
```

- [ ] **Step 5: Task center** (`TaskCenter.tsx`)

```tsx
import { useEffect, useMemo, useState } from "react";
import { Modal, useToast } from "../../ui";
import { useI18n } from "../../i18n";
import { agentTaskLog, cancelAgentTask, isOpenTask, retryAgentTask, subscribeTasks, taskSnapshot, type AgentTask } from "./taskStore";

const STATE_TEXT: Record<AgentTask["state"], string> = {
  queued: "排队中", running: "进行中", succeeded: "已完成", failed: "失败", cancelled: "已取消",
};
const ACTION_TEXT: Record<AgentTask["action"], string> = { install: "安装", update: "更新", uninstall: "卸载", repair: "修复" };

export function TaskCenter() {
  const { t } = useI18n();
  const toast = useToast();
  const [tasks, setTasks] = useState(taskSnapshot());
  const [open, setOpen] = useState(false);
  const [log, setLog] = useState<{ task: AgentTask; lines: string[] } | null>(null);
  useEffect(() => subscribeTasks(setTasks), []);
  const list = useMemo(() => Object.values(tasks).sort((a, b) => b.createdAt.localeCompare(a.createdAt)), [tasks]);
  const running = list.filter(isOpenTask).length;
  if (list.length === 0) return null;

  async function showLog(task: AgentTask) {
    try {
      setLog({ task, lines: await agentTaskLog(task.id) });
    } catch (error) {
      toast(t("读取任务日志失败：") + error, "err");
    }
  }

  return (
    <div className="task-center">
      <button className={"gh sm task-center-btn" + (running ? " busy" : "")} aria-expanded={open} onClick={() => setOpen(!open)}>
        <i className={"ti " + (running ? "ti-loader spin" : "ti-list-check")} />
        {t("任务")}
        {running > 0 && <span className="navdot">{running}</span>}
      </button>
      {open && (
        <div className="task-panel" role="dialog" aria-label={t("任务")}>
          {list.map((task) => (
            <div className={"task-row " + task.state} key={task.id}>
              <div className="task-main">
                <div className="task-title">{task.surfaceLabel} · {t(ACTION_TEXT[task.action])}<span className="task-state">{t(STATE_TEXT[task.state])}</span></div>
                <div className="task-line mono" title={task.message ?? task.lastLine ?? ""}>{task.message ?? task.lastLine ?? ""}</div>
              </div>
              <div className="task-actions">
                {isOpenTask(task) && <button className="gh sm" onClick={() => void cancelAgentTask(task.id)}>{t("取消")}</button>}
                {(task.state === "failed" || task.state === "cancelled") && <button className="gh sm" onClick={() => void retryAgentTask(task.id).catch((error) => toast(String(error), "err"))}>{t("重试")}</button>}
                <button className="gh sm" onClick={() => void showLog(task)}>{t("日志")}</button>
              </div>
            </div>
          ))}
        </div>
      )}
      {log && (
        <Modal title={`${log.task.surfaceLabel} · ${t("任务日志")}`} icon="ti-file-text" wide onClose={() => setLog(null)}>
          <pre className="task-log mono">{log.lines.length ? log.lines.join("\n") : t("暂无日志")}</pre>
        </Modal>
      )}
    </div>
  );
}
```

`agentTasks.css`:

```css
.a .task-center{ position:relative; margin-left:auto; }
.a .task-center-btn .navdot{ position:static; margin-left:4px; }
.a .task-panel{ position:absolute; right:0; top:calc(100% + 6px); width:420px; max-height:420px; overflow:auto;
  background:var(--card); border:1px solid var(--line); border-radius:10px; padding:6px; z-index:40;
  box-shadow:0 12px 32px rgba(0,0,0,.35); }
.a .task-row{ display:flex; gap:8px; align-items:center; padding:8px; border-radius:8px; }
.a .task-row + .task-row{ border-top:1px solid var(--line); }
.a .task-main{ flex:1; min-width:0; }
.a .task-title{ font-size:13px; display:flex; gap:8px; align-items:center; }
.a .task-state{ font-size:11px; color:var(--mut); }
.a .task-row.failed .task-state{ color:var(--err, #e5484d); }
.a .task-row.succeeded .task-state{ color:var(--ok, #30a46c); }
.a .task-line{ font-size:11px; color:var(--mut); white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }
.a .task-actions{ display:flex; gap:4px; }
.a .task-log{ max-height:60vh; overflow:auto; font-size:12px; white-space:pre-wrap; }
```

Check that `--card`, `--line`, `--mut` exist in `src/styles.css`; if a variable has a different name there, use the existing name.

- [ ] **Step 6: Wire into `App.tsx`**

Import `./features/agent-tasks/agentTasks.css`, `TaskCenter`, and `useTaskToasts`. Call `useTaskToasts()` in the component that is rendered inside `ToastProvider` (the same component that renders the sidebar). In the header `.hd`, render `<TaskCenter />` after the title block and before the `page === "overview"` block so it appears on every page.

- [ ] **Step 7: Add translations**

Run `npm run check:i18n`, then add English entries to `src/en.generated.ts` for each reported string: 任务 → Tasks, 排队中 → Queued, 进行中 → Running, 已完成 → Completed, 失败 → Failed, 已取消 → Cancelled, 安装 → Install, 更新 → Update, 卸载 → Uninstall, 修复 → Repair, 完成 → completed, 取消 → Cancel, 重试 → Retry, 日志 → Log, 任务日志 → Task log, 暂无日志 → No log yet, 读取任务日志失败： → Failed to read task log: , 可在任务面板查看日志 → see the task panel for the log. Also add every new Rust user-facing string from Tasks 5-10 that the script reports.

- [ ] **Step 8: Run the frontend checks.** Expected: pass.

- [ ] **Step 9: Commit** — `feat: add the agent task center and completion toasts`.

---

### Task 14: 安装更新 page on tasks, one-click update, broken installs

**Files:**
- Create: `src/features/agents/UpdatePlanModal.tsx`, `src/features/agents/surfaceTask.ts`, `src/features/agents/surfaceTask.test.ts`
- Modify: `src/pages/Agents.tsx`, `src/pages/Agents.test.tsx`, `src/en.generated.ts`

**Interfaces:**
- Consumes: `startAgentTask`, `openTaskFor`, `subscribeTasks`, `taskSnapshot` (Task 13); `VibeTool` with `icon`, `cli_id`, `cli_note`, surface `health`, `broken_reason`, `other_installs`, `can_repair` (Task 12).
- Produces: `surfaceTask.ts`: `export function surfaceTaskText(task: AgentTask | null): string | null` — `"排队中"` for queued, `task.lastLine ?? "正在执行…"` for running, `null` otherwise.

- [ ] **Step 1: Write the failing tests**

`surfaceTask.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { surfaceTaskText } from "./surfaceTask";
import type { AgentTask } from "../agent-tasks/taskStore";

const base = { id: "t", productId: "pi", productName: "pi", surface: "cli", surfaceLabel: "pi", cliId: "pi", action: "update",
  message: null, lastLine: null, createdAt: "", startedAt: null, finishedAt: null } as const;

describe("surface task text", () => {
  it("describes queued and running tasks only", () => {
    expect(surfaceTaskText(null)).toBeNull();
    expect(surfaceTaskText({ ...base, state: "queued" } as AgentTask)).toBe("排队中");
    expect(surfaceTaskText({ ...base, state: "running", lastLine: "npm install" } as AgentTask)).toBe("npm install");
    expect(surfaceTaskText({ ...base, state: "running" } as AgentTask)).toBe("正在执行…");
    expect(surfaceTaskText({ ...base, state: "failed" } as AgentTask)).toBeNull();
  });
});
```

Add to `Agents.test.tsx` a render test for a broken surface. Export `SurfaceState` from `Agents.tsx` (a pure presentational component: badge + meta lines, no buttons), then:

```tsx
it("shows a broken install with its reason and other installs", () => {
  const surface: VibeSurface = {
    available: true, label: "Claude Code CLI", kind: "CLI", description: "", installed: false,
    status: "broken", health: "broken", broken_reason: "不是有效的 Windows 程序", update_available: false,
    install_url: "", docs_url: "", can_install: true, can_update: false, can_uninstall: true, can_open: false,
    can_repair: true, other_installs: [{ path: "C:\\winget\\claude.exe", healthy: true, version: "2.1.227" }],
  };
  const html = renderToStaticMarkup(<SurfaceState surface={surface} />);
  expect(html).toContain("已损坏");
  expect(html).toContain("不是有效的 Windows 程序");
  expect(html).toContain("另有 1 个安装");
});
```

- [ ] **Step 2: Run to verify failure.** Run: `npx vitest run src/features/agents src/pages/Agents.test.tsx`. Expected: FAIL.

- [ ] **Step 3: Implement `surfaceTask.ts`**

```ts
import type { AgentTask } from "../agent-tasks/taskStore";

export function surfaceTaskText(task: AgentTask | null): string | null {
  if (!task) return null;
  if (task.state === "queued") return "排队中";
  if (task.state === "running") return task.lastLine ?? "正在执行…";
  return null;
}
```

- [ ] **Step 4: Rework `Agents.tsx`**

- Remove `useBusy`, the `runBusy` call and the `op_cancel` wiring. `runToolAction(tool, target, action)` becomes:

```ts
async function runToolAction(tool: VibeTool, target: "cli" | "desktop", action: AgentTaskAction) {
  try {
    await startAgentTask(tool.id, target, action);
    setUninstall(null);
    setRepair(null);
  } catch (e) {
    toast(`${translateText("无法创建任务：")}${e}`, "err");
  }
}
```

- Subscribe to tasks: `const [tasks, setTasks] = useState(taskSnapshot()); useEffect(() => subscribeTasks(setTasks), []);`
- In `SurfaceRow`, compute `const task = openTaskFor(tasks, tool.id, target === "cli" ? tool.cli_id : null, target); const taskText = surfaceTaskText(task);`. When `task` is set: disable install/update/uninstall/repair buttons, show `<span className="bd b"><i className="ti ti-loader spin" /> {taskText}</span>` in the title row, and render a `取消` button that calls `cancelAgentTask(task.id)`.
- Replace the badge and meta block with `<SurfaceState surface={surface} />`:

```tsx
export function SurfaceState({ surface }: { surface: VibeSurface }) {
  const others = surface.other_installs?.length ?? 0;
  return (
    <>
      <div className="surface-title" title={surface.label}>
        {surface.label}
        {surface.health === "broken" ? <span className="bd e">已损坏</span> : surfaceBadge(surface)}
        {surface.install_method_label && <span className="bd b" title={installMethodHint(surface)}>{surface.install_method_label}</span>}
      </div>
      <div className="surface-desc" title={surface.description}>{surface.description}</div>
      <div className="surface-meta mono" title={surface.path || ""}>
        <span>{surface.health === "broken" ? `生效入口无法运行：${surface.broken_reason ?? ""}` : surfaceStatusText(surface)}</span>
        {surface.latest ? ` · 最新版本：${surface.latest}` : ""}
        {surface.path ? ` · ${surface.path}` : ""}
        {others > 0 && <span title={surface.other_installs.map((info) => `${info.path}${info.version ? ` (${info.version})` : ""}${info.healthy ? "" : " ✕"}`).join("\n")}>{` · 另有 ${others} 个安装`}</span>}
      </div>
    </>
  );
}
```

  If `.bd.e` does not exist in `styles.css`, add `.a .bd.e{ background:rgba(229,72,77,.14); color:#e5484d; }` next to the other `.bd` variants.
- Add a `修复` button in the CLI row, shown only when `surface.can_repair`, that opens a `ConfirmModal`:

```tsx
{repair && (
  <ConfirmModal
    title={`修复 ${repair.surface.label}`}
    icon="ti-tool"
    danger
    message={<>将卸载无法运行的生效入口 <code>{repair.surface.path}</code>，之后使用本机另一份健康的安装。不会删除账号登录信息、会话或项目文件。</>}
    confirmLabel="确认修复"
    onClose={() => setRepair(null)}
    onConfirm={() => runToolAction(repair.tool, "cli", "repair")}
  />
)}
```

- Under the CLI row, when `tool.cli_note` is set, render `<div className="surface-note"><i className="ti ti-info-circle" /> {tool.cli_note}</div>` (style: `.a .surface-note{ font-size:11px; color:var(--mut); padding:0 12px 8px 64px; }`).
- Brand icons: delete `TOOL_BRAND_ICONS`; use `tool.icon ? <img src={`/brands/${tool.icon}`} alt="" /> : <i className="ti ti-sparkles" />`.
- Page title text `AI办公智能体` → `安装更新`; keep the rest of the hero copy.
- Add a `一键更新` primary button in the hero `.cacts` (before 状态刷新), disabled while `loading`, that opens `UpdatePlanModal`.

- [ ] **Step 5: Implement `UpdatePlanModal.tsx`**

```tsx
import { useEffect, useState } from "react";
import { invoke } from "../../invoke";
import { Loading, Modal, useToast } from "../../ui";
import { translateText } from "../../i18n";
import type { AgentTask } from "../agent-tasks/taskStore";

type PlanItem = { productId: string; productName: string; surface: "cli" | "desktop"; surfaceLabel: string; current?: string | null; latest?: string | null; reason?: string | null };
type UpdatePlan = { auto: PlanItem[]; manual: PlanItem[] };

export function UpdatePlanModal({ onClose }: { onClose: () => void }) {
  const toast = useToast();
  const [plan, setPlan] = useState<UpdatePlan | null>(null);
  const [starting, setStarting] = useState(false);

  useEffect(() => {
    invoke<UpdatePlan>("agent_update_plan")
      .then(setPlan)
      .catch((error) => { toast(`${translateText("读取更新计划失败：")}${error}`, "err"); onClose(); });
  }, [onClose, toast]);

  async function start() {
    setStarting(true);
    try {
      const tasks = await invoke<AgentTask[]>("agent_update_all");
      toast(`${translateText("已创建更新任务")}：${tasks.length}`, "ok");
      onClose();
    } catch (error) {
      toast(`${translateText("创建更新任务失败：")}${error}`, "err");
      setStarting(false);
    }
  }

  const row = (item: PlanItem) => (
    <li key={`${item.productId}-${item.surface}`}>
      <b>{item.surfaceLabel}</b>
      <span className="mono dim">{item.current ?? "?"} → {item.latest ?? "?"}</span>
      {item.reason && <span className="dim">{item.reason}</span>}
    </li>
  );

  return (
    <Modal title="一键更新" icon="ti-cloud-upload" wide onClose={starting ? undefined : onClose}
      footer={<>
        <button className="gh" disabled={starting} onClick={onClose}>取消</button>
        <button className="pr" disabled={!plan || plan.auto.length === 0 || starting} onClick={() => void start()}>
          {starting ? "正在创建任务…" : `开始更新 ${plan?.auto.length ?? 0} 项`}
        </button>
      </>}>
      {!plan ? <Loading /> : (
        <div className="update-plan">
          <div className="seclabel">将在后台更新</div>
          {plan.auto.length ? <ul>{plan.auto.map(row)}</ul> : <p className="dim">没有可自动更新的项目。</p>}
          {plan.manual.length > 0 && <>
            <div className="seclabel">需要手动处理</div>
            <ul>{plan.manual.map(row)}</ul>
          </>}
        </div>
      )}
    </Modal>
  );
}
```

  Check `Loading`'s props in `src/ui.tsx`; if it requires a label, pass `label="正在读取更新计划…"`. Style: `.a .update-plan ul{ list-style:none; padding:0; margin:6px 0 12px; } .a .update-plan li{ display:flex; gap:10px; align-items:baseline; padding:6px 0; border-bottom:1px solid var(--line); }`.

- [ ] **Step 6: Translations.** Run `npm run check:i18n` and add English for every reported string (e.g. 一键更新 → Update All, 将在后台更新 → Will update in the background, 需要手动处理 → Needs manual action, 没有可自动更新的项目。 → Nothing can be updated automatically., 开始更新 {n} 项 via the template form used elsewhere in `en.generated.ts`, 已损坏 → Broken, 修复 → Repair, 确认修复 → Repair, 另有 → also, 生效入口无法运行： → The active entry cannot run: , 无法创建任务： → Cannot create task: ).

- [ ] **Step 7: Run the frontend checks.** Expected: pass.

- [ ] **Step 8: Commit** — `feat: task-driven agent actions, one-click update and repair`.

---

### Task 15: Verification, docs and manual acceptance

**Files:**
- Modify: `docs/development.md` (module table rows for the agent pages and removed work sessions), `README.md`, `README.zh-CN.md` (feature list names: 安装更新 / 会话数据; mention one-click update and background tasks), `docs/superpowers/specs/2026-09-18-agent-catalog-and-task-center-design.md` (status line → `状态：已实现`)

- [ ] **Step 1: Full automated check**

```bash
npm run lint
npm run typecheck
npm run test
npm run check:i18n
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: all pass. Record the Rust and Vitest test counts.

- [ ] **Step 2: Update docs** as listed in Files. In `docs/development.md` replace the rows "AI 智能体目录" and "会话与项目空间" with:

| 范围 | 前端入口 | Rust 模块 | 说明 |
| --- | --- | --- | --- |
| 智能体管理 · 安装更新 | `src/pages/Agents.tsx`、`src/features/agents`、`src/features/agent-tasks` | `agents/` | 注册表、健康检查、安装更新卸载修复、后台任务与一键更新 |
| 智能体管理 · 会话数据 | `src/pages/AgentData.tsx`、`src/features/conversations` | `conversations/` | 本地索引、筛选、批量操作、导出、摘要和交接资料 |

  and replace "智能体更新目前仍是单任务交互…尚未形成统一任务中心" in 已知限制 with "智能体安装任务已并行执行；其他生态页面的安装仍使用单任务弹窗。"

- [ ] **Step 3: Start the dev app** with the `stacker-tauri-dev` entry in `.claude/launch.json` (`npm run tauri dev`) and walk the manual checklist:

1. Sidebar shows 环境体检; 智能体管理 (安装更新, 会话数据); 开发工具链 (Git … Rust); 网络与存储 (终端代理, 磁盘清理); bottom 配置备份, 偏好设置. English locale shows the English labels.
2. A saved last page of `vibe` opens 安装更新.
3. WorkBuddy 中国版 and 国际版 both show CodeBuddy CLI; the CN card shows the shared-CLI note.
4. Qoder cards do not show Qoder IDE as installed.
5. pi card appears; install pi, then update it; the header task button shows progress and a toast appears on completion.
6. Start updates on 2-3 agents at once: they run or queue per the scheduler, logs do not mix, cancelling one leaves the others running.
7. Navigate away during an update; the completion toast still appears.
8. 一键更新 shows the plan with the auto and manual groups; starting it creates tasks.
9. Temporarily create a fake broken entry to see 已损坏: in a temp dir placed first on the user PATH put a `pi.exe` containing text, refresh, confirm the pi row shows 已损坏 with 另有 1 个安装 and a 修复 button. Remove the temp dir afterwards; do not use repair on real installs for this test.
10. Tray 退出 with a running task shows the confirm dialog.

- [ ] **Step 4: Commit**

```bash
git add -A docs README.md README.zh-CN.md
git commit -m "docs: document the rebuilt agent pages and task center

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```
