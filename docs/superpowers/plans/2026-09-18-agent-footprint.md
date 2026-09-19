# Agent Footprint Ledger (C2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show everything Codex and Claude keep on disk, classified as session records / reclaimable / review / keep, and clean what is selected safely.

**Architecture:** A new `src-tauri/src/sessions/footprint/` module. A measurer walks each agent root without following links and deduplicates by Windows file identity. A static rule table maps top-level entries (and a few known subfolders) to classified items; judgement functions use the C1 session catalog (orphan ids) and running process images (versions in use, app running). Cleanup reuses the C1 preview → re-verify → background job shape. The frontend adds an 「占用」 tab to the session catalog.

**Tech Stack:** Rust (winapi Toolhelp32 + QueryFullProcessImageNameW), React + TypeScript, Vitest.

**Spec:** `docs/superpowers/specs/2026-09-18-agent-footprint-design.md`

## Global Constraints

- Never follow symbolic links or junctions; every deleted path must lie inside the root it was found in.
- Deduplicate by file identity (volume serial + file index); MSIX-redirected roots count once.
- Never kill processes; entries owned by a running app are blocked with `E_APP_RUNNING`.
- Newest `claude-code\<version>` and any version with a running `claude.exe` are kept; newest `codex-command-runner-<version>.exe` is kept.
- Codex `bin\<hash>` and `runtimes` are Keep.
- Every Chinese UI string needs English in `src/en.generated.ts`; Rust test fixtures use English text.
- MSRV 1.77.2; `cargo clippy --all-targets -- -D warnings`, `cargo fmt`, `npm run lint`, `npm run typecheck`, `npm run test`, `npm run check:i18n` before each commit (run each separately; do not pipe through `tail` without `pipefail`).
- Commit messages end with `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`.

## File Map

| File | Responsibility |
| --- | --- |
| `src-tauri/src/sessions/footprint/mod.rs` | module wiring |
| `footprint/model.rs` | `FootprintKind`, `Owner`, `FootprintItem`, `AgentFootprint`, `FootprintReport` |
| `footprint/measure.rs` | `Meter` — size of a path with identity dedup, link skipping, warnings |
| `footprint/processes.rs` | `running_images() -> Vec<PathBuf>`; `Running { codex, claude_app, claude_cli_versions }` |
| `footprint/rules.rs` | `Context`, `classify_root(...) -> Vec<Draft>` for every root kind; version helpers |
| `footprint/ledger.rs` | resolve roots, run rules, measure, "other" bucket, compaction estimate, cache |
| `footprint/cleanup.rs` | preview, re-verify, background job |
| `sessions/commands.rs` | `footprint_scan`, `footprint_preview`, `footprint_execute`, `footprint_job` |
| `src/features/sessions/FootprintPanel.tsx`, `FootprintDialog.tsx` | UI |
| `src/features/sessions/types.ts`, `api.ts`, `SessionCatalog.tsx`, `sessions.css` | wiring |

---

### Task 1: Model, measurer, process probe

**Interfaces produced:**

```rust
pub enum FootprintKind { Sessions, Reclaimable, Review, Keep }      // serde lowercase
pub enum Owner { Shared, Cli, DesktopApp }                           // serde snake_case
pub struct FootprintItem { id, agent: Agent, owner: Owner, kind, label: String, explain: String,
    paths: Vec<String>, bytes: u64, files: u64, blocked: Option<String>, note: Option<String> }
pub struct AgentFootprint { agent: Agent, total: u64, reclaimable: u64, items: Vec<FootprintItem> }
pub struct FootprintReport { agents: Vec<AgentFootprint>, total: u64, reclaimable: u64, scanned_at: u64, warnings: Vec<String> }

pub struct Meter { seen: HashSet<FileIdentity>, pub warnings: Vec<String> }
impl Meter { pub fn new() -> Self; pub fn measure(&mut self, path: &Path) -> (u64 /*bytes*/, u64 /*files*/) }

pub fn running_images() -> Vec<PathBuf>
```

`Meter::measure` walks recursively with `symlink_metadata`, skips links/reparse points (`space_analysis::walker::is_link_or_reparse_point`), counts each file once by `space_analysis::windows_fs::file_identity` (falls back to counting when identity fails), and records unreadable directories in `warnings` (display path + `E_ACCESS`). Directory identities are also recorded so a redirected root visited twice contributes nothing the second time.

`running_images` uses `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)`, `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` and `QueryFullProcessImageNameW`; processes that cannot be opened are skipped. Add winapi features `tlhelp32` and `psapi` if needed.

Tests (temp dirs): hard link counted once; the same directory measured twice counts once; a junction/symlink target is skipped (create with `std::os::windows::fs::symlink_dir` when permitted, else `mklink /J` via `cmd`; skip the assertion if creation fails); `running_images()` contains the current test executable.

Commit: `feat: measure agent folders with file identity dedup`.

### Task 2: Rules

**Interfaces produced:**

```rust
pub struct Context<'a> { pub session_ids: &'a HashSet<String>, pub running: &'a [PathBuf], pub now: u64 }
pub enum RootKind { CodexHome, CodexApp, ClaudeHome, ClaudeApp }
pub struct Draft { rule: &'static str, kind, owner, label: String, explain: String, paths: Vec<PathBuf>, blocked: Option<String> }
pub fn classify_root(kind: RootKind, agent_root: &Path, cx: &Context) -> Vec<Draft>
pub fn parse_version(s: &str) -> Option<Vec<u64>>   // "0.146.0-alpha.9.2" → [0,146,0,9,2]; "2.1.275" → [2,1,275]
```

`classify_root` lists the root's top-level entries and returns one `Draft` per rule hit, following the spec tables (§3). Entries not matched by any rule are returned as a single `Draft { rule: "other", kind: Keep }` holding all unmatched paths. Special handling:

- CodexHome `.sandbox-bin`: files named `codex-command-runner-<v>.exe` — all but the highest version → one Reclaimable draft; the rest of `.sandbox-bin` → Keep.
- CodexHome `tmp`: one Review draft per subdirectory; loose files join one Review draft.
- CodexHome `*.log` at root and ClaudeApp `Logs\*.log`: modified more than 7 days before `cx.now` → Reclaimable; recent → Keep.
- CodexHome `logs_*.sqlite` plus its `-wal`/`-shm` → one Review draft.
- ClaudeHome `file-history\<id>` and `session-env\<id>` not in `cx.session_ids` → two Reclaimable drafts; the rest → Keep. `shell-snapshots` files older than 7 days → Reclaimable.
- ClaudeApp `claude-code\<version>`: keep the max version and any version whose folder contains a path in `cx.running`; others → one Reclaimable draft.
- Blocking: CodexHome `.tmp`/`cache`/`logs_*` and every CodexApp/ClaudeApp reclaimable draft get `blocked = Some("E_APP_RUNNING")` when the owning program runs (Codex: any running image whose file name starts with `codex`; Claude app: an image named `Claude.exe` (case-sensitive match on `Claude.exe` vs lowercase `claude.exe` is not reliable, so match any image under `WindowsApps\Claude_` or `AnthropicClaude`); Claude CLI caches: a running `claude.exe`).

Tests: one synthetic tree per root kind asserting kinds, labels' rule ids, version keeping (including running version kept), orphan filtering, 7-day log rule, other bucket.

Commit: `feat: classify agent folders into footprint rules`.

### Task 3: Ledger and scan command

**Interfaces produced:**

```rust
pub fn scan(roots: &Roots, session_ids: &HashSet<String>) -> FootprintReport
#[tauri::command] pub async fn footprint_scan(refresh: bool) -> Result<FootprintReport, String>
```

Roots: CodexHome = `roots.codex`; CodexApp = `%LOCALAPPDATA%\OpenAI\Codex`, `%LOCALAPPDATA%\Packages\OpenAI.Codex_*`; ClaudeHome = `roots.claude`; ClaudeApp = `%APPDATA%\Claude`, `%LOCALAPPDATA%\Claude`, `%LOCALAPPDATA%\Claude-3p`, `%LOCALAPPDATA%\Packages\Claude_*`. The first-listed paths are measured first, so MSIX duplicates vanish under dedup. One `Meter` for the whole scan. Draft → item: measure each path, drop items with 0 bytes, id = `rule:` + sha256 of sorted paths (first 12 hex). Sessions items get `note` = estimated compaction share: sample the 20 largest rollouts, sum `compacted` line bytes / file bytes, apply the ratio to the sessions total, format "其中压缩快照约 {GB}". Session ids come from the C1 catalog (all sessions including automation, plus children native ids). Cache the last report in a `Mutex`; `refresh=false` returns it when present.

Live ignored test `live_footprint` prints totals per agent.

Commit: `feat: build the agent footprint ledger`.

### Task 4: Cleanup

**Interfaces produced:**

```rust
pub struct CleanupPreview { token, items: Vec<FootprintItem>, bytes: u64, blocked: Vec<FootprintItem>, created: u64 }
#[tauri::command] pub async fn footprint_preview(ids: Vec<String>) -> Result<CleanupPreview, String>
#[tauri::command] pub async fn footprint_execute(token: String) -> Result<CleanupJob, String>
#[tauri::command] pub fn footprint_job() -> Option<CleanupJob>
```

Preview rescans (`refresh`), keeps selected ids that are Reclaimable or Review, splits blocked ones out; Sessions/Keep ids → `E_REQUEST`. Execute re-scans; if any selected id is missing, now blocked, or its bytes differ by more than 1 % → `E_CHANGED`. Deletion per path: refuse links and paths outside their root (`canonicalize` + `starts_with`); `remove_dir_all` / `remove_file`; per-item result with freed bytes. Job state shape mirrors C1 (`running|completed|failed`), plus `freed: u64`. Clears the report cache when done.

Tests: deletion refuses a path outside the root; refuses a link; changed bytes abort; freed bytes reported.

Commit: `feat: clean selected agent footprint items`.

### Task 5: Session sort by size

Add `sort: String` (`""` | `"bytes"`) to `SessionQuery`; `catalog::filter` sorts by bytes desc when set. Test in `catalog.rs`. Frontend type gets `sort`.

Commit: `feat: sort sessions by size`.

### Task 6: Frontend 「占用」 tab

- `types.ts`: `FootprintKind`, `FootprintItem`, `AgentFootprint`, `FootprintReport`, `CleanupPreview`, `CleanupJob`; errors `E_APP_RUNNING: "对应程序正在运行，请先退出后再清理。"`.
- `api.ts`: `scanFootprint(refresh)`, `previewCleanup(ids)`, `executeCleanup(token)`, `cleanupJob()`.
- `FootprintPanel.tsx`: header totals + 「清理所选 X」 + 「重新统计」; per agent a card with four sections in order 会话记录 / 可安全清理 / 需你判断 / 保留. Reclaimable rows checked by default (unless blocked); Review rows unchecked, checking one shows its explain text highlighted; Keep and Sessions rows have no checkbox; Sessions row has 「查看最大的会话」 → `onShowSessions()` which sets `sort: "bytes"` and switches to the 会话 tab. Rows expand to list paths.
- `FootprintDialog.tsx`: preview list, blocked list, confirm, progress, results — same pattern as `DeleteDialog`.
- `SessionCatalog.tsx`: tabs 会话 / 项目 / 占用 / 数据来源.
- Test `FootprintPanel.test.tsx`: default selection = reclaimable only; review needs explicit check; sessions row switches tab with sort.

Commit: `feat: add the 占用 tab to 会话数据`.

### Task 7: Docs and verification

- `docs/sessions.md`: add 「占用」 section (categories, rules summary, safety).
- Spec status → 已实现.
- Run `live_footprint` and compare with the spec table.

Commit: `docs: document the agent footprint ledger`.
