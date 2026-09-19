# G3：桥接程序与 Stacker 同步 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 插件与 Stacker 通过 Chrome 本地消息连接：插件的对话索引、已读正文、整理数据、摘录同步进 Stacker 的 `webchat.sqlite3`；Stacker「会话数据」新增「网页对话」标签（列表、搜索、查看、摘要）与「设置 → 浏览器插件」连接页；插件显示同步状态、可从 Stacker 恢复整理数据、连接时导出直接存进 Stacker 导出目录。

**Architecture:** 不另做桥接二进制：`stacker.exe` 的第一个参数以 `chrome-extension://` 开头时进入「桥接模式」，在标准输入输出上跑长度前缀 JSON 循环，不启动 Tauri、不开窗口。Rust 侧新模块 `src-tauri/src/webchat/`：分帧、协议、存储（独立的 `webchat.sqlite3` + gzip 正文文件）、导出、主机登记（HKCU 注册表 + 清单文件）和 Tauri 命令。插件侧 IndexedDB 升到 v2 加 `outbox`，每处本地修改入队；后台脚本持有本地消息端口，按需连接、批量推送；连不上即单独使用。

**Tech Stack:** Rust（rusqlite、flate2、winreg、serde_json）、Tauri 2、React 19 + TypeScript、Vitest（jsdom、fake-indexeddb）、Chrome Manifest V3 `nativeMessaging`。

**Spec:** `docs/superpowers/specs/2026-09-19-web-chat-extension-design.md`（本计划只覆盖分期 G3；G4 提炼不在本期）。本期的约束性裁定（桥接模式、协议、存储、合并规则、界面）以本计划 Global Constraints 为准，它们覆盖设计文档 §3.2 中「独立二进制」的写法（Task 18 同步修改设计文档）。

## Global Constraints

- Rust：`cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` 干净；`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` 通过；`cargo test --manifest-path src-tauri/Cargo.toml` 通过。
- MSRV 1.77.2：不用 `Option::is_none_or`、`LazyLock`、`Iterator::is_sorted` 等 1.78+ API（`is_some_and`、`OnceLock`、`let … else` 可用）。
- 前端与插件：每个任务结束 `npm run typecheck`、`npm run lint`、`npx vitest run`、`npm run check:i18n` 全绿。
- i18n：Stacker 的 `src/` 与 `src-tauri/src` 中每个新中文字面量在 `src/en.generated.ts` 的 `GENERATED_EN` 里有英文（已存在的键不要重复添加）；插件每个新 `t("…")` 在 `extension/src/i18n.ts` 的 `EN` 里有英文；Rust 测试样例用英文文本。
- 不读取任何智能体或网站凭证；不保存账号邮箱。
- 注册表只写当前用户（HKCU），只在用户点击「连接」时写（另有控制者在 Task 19 实机测试中写 Chrome 登记，用户已同意并要求保留）；「断开」删除登记。
- 桥接模式永不启动界面、永不打开窗口：在 `run()` 最开头、`tauri::Builder` 与单实例插件之前处理并 `std::process::exit`。
- 本机主机名 `com.stacker.webchat`；清单 `allowed_origins` 只含 `chrome-extension://<extension/EXTENSION_ID>/`；桥接模式只接受这一个来源。
- 协议：双向都是 4 字节小端长度 + UTF-8 JSON，单条最大 1 MiB（1048576 字节）。请求 `{ id, type, payload }`，响应 `{ id, ok, result | error }`。类型：`hello`、`syncAccounts`、`syncConversations`（每批 ≤ 200）、`syncBody`（一条消息一段正文，超过约 900 KB 按消息序号分段）、`syncFolders`、`syncExcerpts`、`removeRecords`、`pullBackup`、`saveExport`、`status`。
- 存储：`<data>` = `%LOCALAPPDATA%\Stacker\<dev|stable>\conversations`（与 `sessions.sqlite3` 同目录，不改其表）；新库 `<data>\webchat.sqlite3`，表 `web_accounts`、`web_conversations`、`web_folders`、`web_excerpts`（另有 `web_meta`、`web_body_parts`）；正文 `<data>\webchat\bodies\<site>\<account>\<id>.json.gz`；清单 `<data>\native-messaging\com.stacker.webchat.json`；网页导出 `<data>\exports\web\`。
- 合并规则：站点字段（标题、时间、归档、删除）以较新的 `listedAt` 为准；本地字段（文件夹、标签、收藏、备注、账号备注名）带 `localUpdatedAt`，整条记录较新者生效。
- 站点 id 在 Stacker 侧一律当字符串处理（G2 并行新增 gemini、grok、deepseek），Stacker 不枚举站点。
- 插件：连接时导出走 `saveExport` 存进 Stacker 导出目录，未连接时照旧用 `chrome.downloads`；管理页状态文字「已连接 Stacker · 待同步 N 项」/「未连接 Stacker」。
- 用 Write/Edit 工具写含反斜杠的内容，不用 bash heredoc。
- 提交信息以 `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` 结尾。

## 本计划补充的决定

1. 数据目录复用 `sessions::annotations::root()`；仅调试构建支持环境变量 `STACKER_WEBCHAT_DIR` 覆盖（供命令行探针用临时目录，正式版忽略）。
2. 插件记录新增 `listedAt`（对话）与 `localUpdatedAt`（账号、对话、文件夹、摘录），作为合并时间戳；v1 旧记录缺省按 0。
3. `outbox` 只存「种类 + 主键」，发送时读取记录当前状态，同一记录多次修改只发一次；v2 升级时入队一条 `all`（把升级前的数据全部推给 Stacker）；`hello` 显示 Stacker 一条对话都没有而本地有时，也入队 `all`。
4. 文件夹、摘录的删除在 Stacker 里记为墓碑（`deleted_at`），之后同一 id 不会复活；对话的删除是站点字段 `removedAt`，随 `syncConversations` 同步。
5. 正文分段先暂存在 `web_body_parts`，收齐后写文件；较旧的一次读取不会覆盖较新的正文；单条消息超过 25 万字符时截断并标注 `…[truncated]`。
6. `pullBackup` 按「分区 + 偏移」分页：`{ section: "accounts"|"folders"|"conversations"|"excerpts", offset }` → `{ items, next }`，每页 JSON ≤ 800 KB。
7. `saveExport { path, text, append }`：只写 `<exports>\web\` 之下；路径最多 4 段、只允许 `.md`/`.json`、逐段用会话导出同一套字符清洗；同名时加「 (n)」；`append` 只能续写本次桥接进程创建的文件；插件把超过 25 万字符的文本分段续写。
8. 打包：`bundle.resources` 放在单独的 `src-tauri/tauri.bundle.conf.json`，只由发布脚本传给 `tauri build`（`extension/dist` 不入库，放进主配置会让普通 `cargo build/test` 因资源不存在而失败）；资源落在 `<exe 目录>\extension`；免安装版 zip 同样复制；开发时指向 `<repo>\extension\dist`。
9. 「断开」某浏览器时删除它的注册表项；只有两个浏览器都不再指向清单文件时才删除清单文件。
10. 网页对话摘要：执行者设置为「同源」时用 Claude（网页对话没有自己的智能体），「固定 Codex / Claude」照设置；摘要存在 `webchat.sqlite3`，正文更新后标记过期；同一时间只跑一个（`E_SUMMARY_BUSY`），可取消。
11. 桥接进程退出码：0 浏览器关闭管道；1 分帧或读写错误（含超过 1 MiB 的帧）；2 来源不符；3 无法打开存储。
12. 「网页对话」标签：搜索标题、备注、标签、摘要；勾选「搜索正文」时才解压正文逐条比对。与本机会话的合并搜索不在本期（按裁定做独立标签）。
13. 插件端口：空闲 2 分钟关闭；连接失败后 30 秒内不重试（管理页「重新连接」可立即重试）；管理页每 5 秒刷新一次状态。
14. 连接时导出失败不回退到下载目录（避免重复文件），只在未连接时用下载目录。

## 文件结构

```
src-tauri/
  Cargo.toml                         + flate2
  tauri.bundle.conf.json             新：bundle.resources（仅发布脚本使用）
  src/lib.rs                         桥接模式入口、mod webchat、命令注册
  src/sessions/commands.rs           blocking、explorer 改为 pub(crate)
  src/sessions/export.rs             safe 改为 pub(crate)
  src/webchat/
    mod.rs                           数据目录、插件 ID、来源、毫秒时间
    framing.rs                       4 字节小端长度 + JSON 的读写
    protocol.rs                      请求、响应、同步记录类型与校验
    store.rs                         webchat.sqlite3：迁移、合并写入、分页、列表与搜索、摘要
    bodies.rs                        gzip 正文文件与路径清洗
    export.rs                        saveExport：路径校验与写入
    bridge.rs                        桥接模式：参数识别、读写循环、分发
    host.rs                          清单与 Chrome / Edge 注册表登记、插件文件夹定位
    commands.rs                      「网页对话」与「浏览器插件」的 Tauri 命令
scripts/
  webchat-bridge-probe.mjs           新：像浏览器一样从命令行驱动桥接模式
  release-windows.ps1                构建插件、带资源配置打包、免安装版复制插件
src/
  en.generated.ts                    新英文
  features/sessions/
    api.ts  types.ts  sessions.css
    SessionCatalog.tsx               新标签「网页对话」
    SettingsPanel.tsx                插入「浏览器插件」
    BrowserExtension.tsx (+ .test)   新：浏览器插件设置块
    WebChatPanel.tsx (+ .test)       新：网页对话列表与搜索
    WebChatDetail.tsx                新：网页对话详情与摘要
extension/
  public/manifest.json               + nativeMessaging
  src/manifest.test.ts
  src/i18n.ts
  src/background.ts                  端口、刷新队列、消息处理
  src/lib/db.ts (+ .test)            v2：outbox、时间戳、入队、恢复
  src/lib/sync.ts (+ .test)          分批、正文分段、flush
  src/lib/bridge.ts (+ .test)        本地消息端口管理
  src/lib/bridgeMessages.ts (+ .test) 页面 ↔ 后台消息与处理器
  src/lib/restore.ts (+ .test)       从 Stacker 恢复
  src/lib/save.ts (+ .test)          导出目标选择与分段写入
  src/ui/errors.ts
  src/ui/manage/App.tsx  main.tsx
  src/ui/manage/SyncStatus.tsx (+ .test)
  src/ui/popup/main.tsx
  README.md
docs/sessions.md
docs/superpowers/specs/2026-09-19-web-chat-extension-design.md   §3.2 改为桥接模式
```

---

### Task 1: webchat 模块骨架与分帧

**Files:**
- Create: `src-tauri/src/webchat/mod.rs`, `src-tauri/src/webchat/framing.rs`
- Modify: `src-tauri/src/lib.rs`（模块声明）

**Interfaces:**
- Produces:
  - `webchat::root() -> PathBuf`；`webchat::export_dir() -> PathBuf`；`webchat::extension_id() -> &'static str`；`webchat::allowed_origin() -> String`；`webchat::now_ms() -> i64`（`pub(crate)`）。
  - `webchat::framing::MAX_MESSAGE: usize = 1_048_576`；`read_message(r: &mut impl Read) -> io::Result<Option<Vec<u8>>>`（管道在两条消息之间关闭时 `Ok(None)`）；`write_message(w: &mut impl Write, body: &[u8]) -> io::Result<()>`。

- [ ] **Step 1: 写失败的测试**

先建 `src-tauri/src/webchat/framing.rs`，放入 Step 3 的常量与两个函数签名，函数体暂写 `unimplemented!()`，再接上下面的测试模块（`mod.rs` 同理：先声明 `pub mod framing;` 与 Step 3 的函数，`extension_id` 暂写 `unimplemented!()`），并在 `lib.rs` 加上 Step 3 所列的模块声明。测试部分：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn framed(body: &[u8]) -> Vec<u8> {
        let mut out = (body.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn reads_messages_until_the_pipe_closes() {
        let mut input = framed(br#"{"a":1}"#);
        input.extend(framed(b"{}"));
        let mut reader = Cursor::new(input);
        assert_eq!(read_message(&mut reader).unwrap().unwrap(), br#"{"a":1}"#);
        assert_eq!(read_message(&mut reader).unwrap().unwrap(), b"{}");
        assert!(read_message(&mut reader).unwrap().is_none());
    }

    #[test]
    fn a_cut_off_message_is_an_error() {
        let mut half_header = Cursor::new(vec![5u8, 0]);
        assert!(read_message(&mut half_header).is_err());
        let mut short_body = Cursor::new(framed(b"hello")[..6].to_vec());
        assert!(read_message(&mut short_body).is_err());
    }

    #[test]
    fn refuses_oversized_messages_both_ways() {
        let mut huge = Cursor::new(((MAX_MESSAGE + 1) as u32).to_le_bytes().to_vec());
        let err = read_message(&mut huge).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        let mut out = Vec::new();
        assert!(write_message(&mut out, &vec![b'x'; MAX_MESSAGE + 1]).is_err());
        assert!(out.is_empty(), "nothing is written for a refused message");
    }

    #[test]
    fn writes_little_endian_length_then_body() {
        let mut out = Vec::new();
        write_message(&mut out, b"hi").unwrap();
        assert_eq!(out, vec![2, 0, 0, 0, b'h', b'i']);
    }
}
```

`src-tauri/src/webchat/mod.rs` 的测试：

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn origin_uses_the_recorded_extension_id() {
        let id = super::extension_id();
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| ('a'..='p').contains(&c)));
        assert_eq!(
            super::allowed_origin(),
            format!("chrome-extension://{id}/")
        );
    }
}
```

- [ ] **Step 2: 运行，确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib webchat::`
Expected: FAIL（`not implemented` panic）。

- [ ] **Step 3: 实现**

`src-tauri/src/webchat/framing.rs`（测试模块接在文件末尾）：

```rust
//! Chrome native messaging framing: a 4-byte little-endian length, then UTF-8 JSON.
use std::io::{self, Read, Write};

/// Largest message either side may send (Chrome caps host → extension at 1 MB).
pub const MAX_MESSAGE: usize = 1024 * 1024;

/// Reads one message; `Ok(None)` when the browser closed the pipe between messages.
pub fn read_message(reader: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut header = [0u8; 4];
    let mut got = 0;
    while got < header.len() {
        match reader.read(&mut header[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    let len = u32::from_le_bytes(header) as usize;
    if len > MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

pub fn write_message(writer: &mut impl Write, body: &[u8]) -> io::Result<()> {
    if body.len() > MAX_MESSAGE {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "message too large"));
    }
    writer.write_all(&(body.len() as u32).to_le_bytes())?;
    writer.write_all(body)?;
    writer.flush()
}
```

`src-tauri/src/webchat/mod.rs`（测试模块接在文件末尾）：

```rust
//! Web chats from the Stacker browser extension: native-messaging bridge, storage and sync.
pub mod framing;

use std::path::PathBuf;

/// Where web chats live: the same folder as `sessions.sqlite3`.
/// Debug builds honour `STACKER_WEBCHAT_DIR` so the bridge can be probed against a scratch folder.
pub fn root() -> PathBuf {
    if cfg!(debug_assertions) {
        if let Some(dir) = std::env::var_os("STACKER_WEBCHAT_DIR").filter(|d| !d.is_empty()) {
            return PathBuf::from(dir);
        }
    }
    crate::sessions::annotations::root()
}

/// Stacker's export folder; web chat exports go into its `web` subfolder.
pub fn export_dir() -> PathBuf {
    root().join("exports")
}

/// The extension's fixed ID (derived from the manifest key), recorded in `extension/EXTENSION_ID`.
pub fn extension_id() -> &'static str {
    include_str!("../../../extension/EXTENSION_ID").trim()
}

/// The only caller the bridge and the host manifest accept.
pub fn allowed_origin() -> String {
    format!("chrome-extension://{}/", extension_id())
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
```

`src-tauri/src/lib.rs`：在 `mod versions;` 之后加（`allow` 在 Task 9 接好命令后删除）：

```rust
// Wired into bridge mode (Task 7) and Tauri commands (Task 9); the allow goes away in Task 9.
#[allow(dead_code)]
mod webchat;
```

- [ ] **Step 4: 运行，确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib webchat::`
Expected: PASS（5 个测试）。再跑 `cargo fmt --manifest-path src-tauri/Cargo.toml` 与 `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/webchat/mod.rs src-tauri/src/webchat/framing.rs src-tauri/src/lib.rs
git commit -m "feat(webchat): native messaging framing" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 2: 协议类型

**Files:**
- Create: `src-tauri/src/webchat/protocol.rs`
- Modify: `src-tauri/src/webchat/mod.rs`（`pub mod protocol;`）

**Interfaces:**
- Consumes: 无。
- Produces（全部 `pub`，serde 字段名 camelCase，时间一律毫秒 `i64`，接受小数并四舍五入）：
  - `PROTOCOL_VERSION: u32 = 1`；`MAX_BATCH: usize = 200`。
  - `Request { id: String, kind: String /* JSON "type" */, payload: Value }`；`Response { id, ok, result: Option<Value>, error: Option<String> }`，`Response::success(id: String, result: Value)`、`Response::failure(id: String, code: &str)`。
  - `WebAccount { key, site, remote_id, name, alias, last_seen: i64, local_updated_at: i64 }`
  - `WebConversation { key, site, account, id, title, created_at, updated_at, archived: bool, removed_at: Option<i64>, listed_at, folder_id: Option<String>, tags: Vec<String>, favorite: bool, note, local_updated_at }`
  - `WebFolder { id, name, created_at, local_updated_at }`
  - `WebExcerpt { id, site, conversation_id: Option<String>, url, page_title, text, note, created_at, local_updated_at }`
  - `WebMessage { role, text, at: Option<i64>, attachments: Vec<String> }`
  - `BodyChunk { key, site, account, id, title, updated_at, fetched_at, chunk: usize, chunks: usize, messages: Vec<WebMessage> }`
  - `StoredBody { key, site, account, id, title, updated_at, fetched_at, messages: Vec<WebMessage> }`
  - `Removal { kind: String /* "folder" | "excerpt" */, key: String, at: i64 }`
  - `Items<T> { items: Vec<T> }`；`PullRequest { section: String, offset: usize }`；`SaveExport { path: String, text: String, append: bool }`
  - `is_site(site: &str) -> bool`；`valid_key(site: &str, id: &str, key: &str) -> bool`

- [ ] **Step 1: 写失败的测试**（接在 `protocol.rs` 末尾）

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn browser_numbers_become_whole_milliseconds() {
        let c: WebConversation = serde_json::from_value(json!({
            "key": "chatgpt:a", "site": "chatgpt", "account": "chatgpt:u1", "id": "a",
            "title": "Trip", "createdAt": 1.4, "updatedAt": 1_700_000_000_000.6_f64,
            "removedAt": null, "tags": ["x"], "favorite": true
        }))
        .unwrap();
        assert_eq!((c.created_at, c.updated_at), (1, 1_700_000_000_001));
        assert_eq!(c.removed_at, None);
        assert_eq!((c.listed_at, c.local_updated_at), (0, 0), "missing times default to 0");
        assert!(c.favorite && c.folder_id.is_none());
    }

    #[test]
    fn requests_may_omit_payload_and_responses_omit_empty_fields() {
        let r: Request = serde_json::from_str(r#"{"id":"1","type":"status"}"#).unwrap();
        assert_eq!((r.id.as_str(), r.kind.as_str()), ("1", "status"));
        assert!(r.payload.is_null());
        let ok = serde_json::to_value(Response::success("1".into(), json!({"a": 1}))).unwrap();
        assert_eq!(ok, json!({"id": "1", "ok": true, "result": {"a": 1}}));
        let err = serde_json::to_value(Response::failure("2".into(), "E_PATH")).unwrap();
        assert_eq!(err, json!({"id": "2", "ok": false, "error": "E_PATH"}));
    }

    #[test]
    fn site_ids_and_keys_are_checked() {
        assert!(is_site("chatgpt") && is_site("deepseek") && is_site("x-1"));
        assert!(!is_site("") && !is_site("Chat") && !is_site("a/b") && !is_site(&"a".repeat(33)));
        assert!(valid_key("claude", "c1", "claude:c1"));
        assert!(!valid_key("claude", "c1", "chatgpt:c1"));
        assert!(!valid_key("claude", "", "claude:"));
        assert!(!valid_key("Claude", "c1", "Claude:c1"));
    }
}
```

- [ ] **Step 2: 运行，确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib webchat::protocol`
Expected: FAIL（类型未定义，编译错误）。

- [ ] **Step 3: 实现**

`src-tauri/src/webchat/mod.rs`：`pub mod framing;` 下一行加 `pub mod protocol;`。

`src-tauri/src/webchat/protocol.rs`（测试模块之前）：

```rust
//! Messages between the extension and the bridge: `{ id, type, payload }` → `{ id, ok, result | error }`.
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;
/// Records per sync message.
pub const MAX_BATCH: usize = 200;

#[derive(Debug, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Response {
    pub id: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn success(id: String, result: Value) -> Self {
        Self { id, ok: true, result: Some(result), error: None }
    }

    pub fn failure(id: String, code: &str) -> Self {
        Self { id, ok: false, result: None, error: Some(code.to_string()) }
    }
}

/// Browser times are JavaScript numbers in milliseconds, sometimes fractional.
fn millis<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    Ok(Option::<f64>::deserialize(d)?.map_or(0, |v| v.round() as i64))
}

fn millis_opt<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    Ok(Option::<f64>::deserialize(d)?.map(|v| v.round() as i64))
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebAccount {
    pub key: String,
    pub site: String,
    pub remote_id: String,
    pub name: String,
    pub alias: String,
    #[serde(deserialize_with = "millis")]
    pub last_seen: i64,
    #[serde(deserialize_with = "millis")]
    pub local_updated_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebConversation {
    pub key: String,
    pub site: String,
    pub account: String,
    pub id: String,
    pub title: String,
    #[serde(deserialize_with = "millis")]
    pub created_at: i64,
    #[serde(deserialize_with = "millis")]
    pub updated_at: i64,
    pub archived: bool,
    #[serde(deserialize_with = "millis_opt")]
    pub removed_at: Option<i64>,
    /// When the site's own fields were last written; the newest listing wins.
    #[serde(deserialize_with = "millis")]
    pub listed_at: i64,
    pub folder_id: Option<String>,
    pub tags: Vec<String>,
    pub favorite: bool,
    pub note: String,
    /// When a local field last changed; the newer record wins.
    #[serde(deserialize_with = "millis")]
    pub local_updated_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebFolder {
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "millis")]
    pub created_at: i64,
    #[serde(deserialize_with = "millis")]
    pub local_updated_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebExcerpt {
    pub id: String,
    pub site: String,
    pub conversation_id: Option<String>,
    pub url: String,
    pub page_title: String,
    pub text: String,
    pub note: String,
    #[serde(deserialize_with = "millis")]
    pub created_at: i64,
    #[serde(deserialize_with = "millis")]
    pub local_updated_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebMessage {
    pub role: String,
    pub text: String,
    #[serde(deserialize_with = "millis_opt")]
    pub at: Option<i64>,
    pub attachments: Vec<String>,
}

/// One piece of a body read; a body over ~900 KB arrives in several, split by message index.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BodyChunk {
    pub key: String,
    pub site: String,
    pub account: String,
    pub id: String,
    pub title: String,
    #[serde(deserialize_with = "millis")]
    pub updated_at: i64,
    #[serde(deserialize_with = "millis")]
    pub fetched_at: i64,
    pub chunk: usize,
    pub chunks: usize,
    pub messages: Vec<WebMessage>,
}

/// A whole body as stored in `<id>.json.gz`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StoredBody {
    pub key: String,
    pub site: String,
    pub account: String,
    pub id: String,
    pub title: String,
    pub updated_at: i64,
    pub fetched_at: i64,
    pub messages: Vec<WebMessage>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Removal {
    /// "folder" | "excerpt"
    pub kind: String,
    pub key: String,
    #[serde(deserialize_with = "millis")]
    pub at: i64,
}

#[derive(Debug, Deserialize)]
pub struct Items<T> {
    #[serde(default = "Vec::new")]
    pub items: Vec<T>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct PullRequest {
    pub section: String,
    pub offset: usize,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SaveExport {
    pub path: String,
    pub text: String,
    pub append: bool,
}

/// Site ids are plain lowercase words (`chatgpt`, `claude`, `gemini`, …); Stacker does not list them.
pub fn is_site(site: &str) -> bool {
    !site.is_empty()
        && site.len() <= 32
        && site
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Conversation keys are `<site>:<id>`, exactly as the extension builds them.
pub fn valid_key(site: &str, id: &str, key: &str) -> bool {
    is_site(site) && !id.is_empty() && id.len() <= 200 && key == format!("{site}:{id}")
}
```

- [ ] **Step 4: 运行，确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib webchat::protocol`
Expected: PASS（3 个测试）。`cargo fmt` 后再跑 clippy。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/webchat/protocol.rs src-tauri/src/webchat/mod.rs
git commit -m "feat(webchat): bridge protocol types" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 3: webchat.sqlite3 与合并写入

**Files:**
- Create: `src-tauri/src/webchat/store.rs`
- Modify: `src-tauri/src/webchat/mod.rs`（`pub mod store;`）

**Interfaces:**
- Consumes: Task 2 的记录类型、`is_site`、`valid_key`。
- Produces（`webchat::store`）：
  - `open(root: &Path) -> Result<Connection, String>`（建目录、WAL、busy_timeout 5 s、按 `PRAGMA user_version` 迁移）
  - `meta(conn: &Connection, key: &str) -> Option<i64>`；`set_meta(conn: &Connection, key: &str, value: i64) -> Result<(), String>`
  - `upsert_accounts(conn, items: &[WebAccount]) -> Result<usize, String>`；`upsert_conversations(conn, &[WebConversation]) -> Result<usize, String>`；`upsert_folders(conn, &[WebFolder]) -> Result<usize, String>`；`upsert_excerpts(conn, &[WebExcerpt]) -> Result<usize, String>`；`remove_records(conn, &[Removal]) -> Result<usize, String>`（返回值 = 接受的条数）
  - `accounts(conn) -> Result<Vec<WebAccount>, String>`；`conversations(conn) -> Result<Vec<WebConversation>, String>`；`folders(conn) -> Result<Vec<WebFolder>, String>`（不含已删）；`excerpts(conn) -> Result<Vec<WebExcerpt>, String>`（不含已删）；都按主键排序
  - `Counts { accounts, conversations, bodies, folders, excerpts: i64 }`（Serialize camelCase）；`counts(conn) -> Result<Counts, String>`
  - 错误码：存储失败 `"E_STORAGE"`。

- [ ] **Step 1: 写失败的测试**（接在 `store.rs` 末尾）

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn conv(id: &str, listed_at: i64, local_at: i64) -> WebConversation {
        WebConversation {
            key: format!("chatgpt:{id}"),
            site: "chatgpt".into(),
            account: "chatgpt:u1".into(),
            id: id.into(),
            title: format!("Title {id}"),
            created_at: 1,
            updated_at: 2,
            listed_at,
            local_updated_at: local_at,
            ..Default::default()
        }
    }

    #[test]
    fn migrations_run_once() {
        let dir = tempfile::tempdir().unwrap();
        drop(open(dir.path()).unwrap());
        let conn = open(dir.path()).unwrap();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        set_meta(&conn, "last_sync_at", 42).unwrap();
        assert_eq!(meta(&conn, "last_sync_at"), Some(42));
        assert_eq!(meta(&conn, "missing"), None);
    }

    #[test]
    fn accounts_keep_the_newest_name_and_the_newest_alias() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        let a = WebAccount {
            key: "chatgpt:u1".into(),
            site: "chatgpt".into(),
            remote_id: "u1".into(),
            name: "Ada".into(),
            alias: String::new(),
            last_seen: 10,
            local_updated_at: 0,
        };
        assert_eq!(upsert_accounts(&conn, &[a.clone()]).unwrap(), 1);
        let renamed = WebAccount {
            name: "Old name".into(),
            last_seen: 5,
            alias: "Work".into(),
            local_updated_at: 20,
            ..a.clone()
        };
        upsert_accounts(&conn, &[renamed]).unwrap();
        let wrong_key = WebAccount { key: "claude:u1".into(), ..a.clone() };
        assert_eq!(upsert_accounts(&conn, &[wrong_key]).unwrap(), 0);
        let saved = accounts(&conn).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!((saved[0].name.as_str(), saved[0].alias.as_str()), ("Ada", "Work"));
        assert_eq!((saved[0].last_seen, saved[0].local_updated_at), (10, 20));
    }

    #[test]
    fn newer_listing_wins_site_fields_and_newer_edit_wins_local_fields() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        let mut first = conv("a", 10, 10);
        first.note = "first note".into();
        upsert_conversations(&conn, &[first]).unwrap();
        // An older listing carrying a newer local edit: the title stays, the local fields change.
        let mut edit = conv("a", 5, 20);
        edit.title = "Old title".into();
        edit.note = "edited".into();
        edit.favorite = true;
        edit.tags = vec!["work".into()];
        edit.folder_id = Some("f1".into());
        upsert_conversations(&conn, &[edit]).unwrap();
        // A newer listing carrying an older local copy: the title changes, the local fields stay.
        let mut relist = conv("a", 30, 15);
        relist.title = "Renamed".into();
        relist.removed_at = Some(30);
        relist.note = "stale".into();
        upsert_conversations(&conn, &[relist]).unwrap();
        let saved = conversations(&conn).unwrap().remove(0);
        assert_eq!(
            (saved.title.as_str(), saved.removed_at, saved.listed_at),
            ("Renamed", Some(30), 30)
        );
        assert_eq!(
            (saved.note.as_str(), saved.favorite, saved.local_updated_at),
            ("edited", true, 20)
        );
        assert_eq!(saved.tags, vec!["work".to_string()]);
        assert_eq!(saved.folder_id.as_deref(), Some("f1"));
    }

    #[test]
    fn invalid_conversations_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        let mut bad_key = conv("a", 1, 1);
        bad_key.key = "chatgpt:b".into();
        let mut bad_site = conv("c", 1, 1);
        bad_site.site = "Chat GPT".into();
        bad_site.key = "Chat GPT:c".into();
        assert_eq!(upsert_conversations(&conn, &[bad_key, bad_site, conv("d", 1, 1)]).unwrap(), 1);
        assert_eq!(counts(&conn).unwrap().conversations, 1);
    }

    #[test]
    fn deleted_folders_and_excerpts_stay_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        let folder = WebFolder { id: "f1".into(), name: "Trips".into(), created_at: 1, local_updated_at: 5 };
        upsert_folders(&conn, &[folder.clone()]).unwrap();
        let renamed = WebFolder { name: "Travel".into(), local_updated_at: 6, ..folder.clone() };
        upsert_folders(&conn, &[renamed]).unwrap();
        assert_eq!(folders(&conn).unwrap()[0].name, "Travel");
        let excerpt = WebExcerpt {
            id: "e1".into(),
            site: "chatgpt".into(),
            conversation_id: Some("a".into()),
            text: "tip".into(),
            created_at: 1,
            local_updated_at: 1,
            ..Default::default()
        };
        let bad_excerpt = WebExcerpt { id: "e2".into(), site: "Bad Site".into(), ..excerpt.clone() };
        assert_eq!(upsert_excerpts(&conn, &[excerpt, bad_excerpt]).unwrap(), 1);
        let removed = remove_records(
            &conn,
            &[
                Removal { kind: "folder".into(), key: "f1".into(), at: 9 },
                Removal { kind: "bogus".into(), key: "x".into(), at: 9 },
            ],
        )
        .unwrap();
        assert_eq!(removed, 1);
        let later = WebFolder { name: "Back".into(), local_updated_at: 99, ..folder };
        upsert_folders(&conn, &[later]).unwrap();
        assert!(folders(&conn).unwrap().is_empty(), "a deleted folder never comes back");
        let c = counts(&conn).unwrap();
        assert_eq!((c.folders, c.excerpts, c.bodies), (0, 1, 0));
    }
}
```

- [ ] **Step 2: 运行，确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib webchat::store`
Expected: FAIL（编译错误：函数未定义）。

- [ ] **Step 3: 实现**

`src-tauri/src/webchat/mod.rs`：加 `pub mod store;`。

`src-tauri/src/webchat/store.rs`（测试模块之前）：

```rust
//! `webchat.sqlite3`: accounts, conversations, folders and excerpts synced from the extension.
//! Kept apart from `sessions.sqlite3`; bodies live in gzip files (see `bodies`).
use super::protocol::*;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;

const FILE: &str = "webchat.sqlite3";

/// Schema versions, applied in order; `PRAGMA user_version` records how many ran.
const MIGRATIONS: &[&str] = &["
CREATE TABLE web_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE web_accounts (
  key TEXT PRIMARY KEY, site TEXT NOT NULL, remote_id TEXT NOT NULL,
  name TEXT NOT NULL DEFAULT '', alias TEXT NOT NULL DEFAULT '',
  last_seen INTEGER NOT NULL DEFAULT 0, local_updated_at INTEGER NOT NULL DEFAULT 0);
CREATE TABLE web_conversations (
  key TEXT PRIMARY KEY, site TEXT NOT NULL, account TEXT NOT NULL, id TEXT NOT NULL,
  title TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL DEFAULT 0,
  archived INTEGER NOT NULL DEFAULT 0, removed_at INTEGER, listed_at INTEGER NOT NULL DEFAULT 0,
  folder_id TEXT, tags TEXT NOT NULL DEFAULT '[]', favorite INTEGER NOT NULL DEFAULT 0,
  note TEXT NOT NULL DEFAULT '', local_updated_at INTEGER NOT NULL DEFAULT 0,
  body_fetched_at INTEGER, body_updated_at INTEGER, body_messages INTEGER NOT NULL DEFAULT 0,
  summary TEXT NOT NULL DEFAULT '', summary_by TEXT NOT NULL DEFAULT '',
  summary_at INTEGER NOT NULL DEFAULT 0, summary_body_at INTEGER);
CREATE INDEX web_conversations_account ON web_conversations(account);
CREATE TABLE web_folders (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at INTEGER NOT NULL DEFAULT 0,
  local_updated_at INTEGER NOT NULL DEFAULT 0, deleted_at INTEGER);
CREATE TABLE web_excerpts (
  id TEXT PRIMARY KEY, site TEXT NOT NULL, conversation_id TEXT, url TEXT NOT NULL DEFAULT '',
  page_title TEXT NOT NULL DEFAULT '', text TEXT NOT NULL, note TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL DEFAULT 0, local_updated_at INTEGER NOT NULL DEFAULT 0, deleted_at INTEGER);
CREATE TABLE web_body_parts (
  key TEXT NOT NULL, fetched_at INTEGER NOT NULL, chunk INTEGER NOT NULL, chunks INTEGER NOT NULL,
  messages TEXT NOT NULL, PRIMARY KEY (key, fetched_at, chunk));
"];

fn db_err<E>(_: E) -> String {
    "E_STORAGE".into()
}

pub fn open(root: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(root).map_err(db_err)?;
    let conn = Connection::open(root.join(FILE)).map_err(db_err)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(db_err)?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(db_err)?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<(), String> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(db_err)?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version.max(0) as usize) {
        let tx = conn.unchecked_transaction().map_err(db_err)?;
        tx.execute_batch(sql).map_err(db_err)?;
        tx.execute_batch(&format!("PRAGMA user_version = {}", i + 1))
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
    }
    Ok(())
}

pub fn meta(conn: &Connection, key: &str) -> Option<i64> {
    conn.query_row("SELECT value FROM web_meta WHERE key=?1", [key], |r| {
        r.get::<_, String>(0)
    })
    .optional()
    .ok()
    .flatten()
    .and_then(|v| v.parse().ok())
}

pub fn set_meta(conn: &Connection, key: &str, value: i64) -> Result<(), String> {
    conn.execute(
        "INSERT INTO web_meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value.to_string()],
    )
    .map(|_| ())
    .map_err(db_err)
}

// In an UPDATE, SQLite evaluates every SET expression against the old row, so the CASEs
// compare the incoming time with the stored one before either is replaced.
const ACCOUNT_UPSERT: &str = "
INSERT INTO web_accounts(key,site,remote_id,name,alias,last_seen,local_updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7)
ON CONFLICT(key) DO UPDATE SET
  name = CASE WHEN excluded.last_seen >= web_accounts.last_seen THEN excluded.name ELSE web_accounts.name END,
  last_seen = MAX(web_accounts.last_seen, excluded.last_seen),
  alias = CASE WHEN excluded.local_updated_at >= web_accounts.local_updated_at THEN excluded.alias ELSE web_accounts.alias END,
  local_updated_at = MAX(web_accounts.local_updated_at, excluded.local_updated_at)";

const CONVERSATION_UPSERT: &str = "
INSERT INTO web_conversations(key,site,account,id,title,created_at,updated_at,archived,removed_at,listed_at,
  folder_id,tags,favorite,note,local_updated_at)
VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
ON CONFLICT(key) DO UPDATE SET
  account = CASE WHEN excluded.listed_at >= web_conversations.listed_at THEN excluded.account ELSE web_conversations.account END,
  title = CASE WHEN excluded.listed_at >= web_conversations.listed_at THEN excluded.title ELSE web_conversations.title END,
  created_at = CASE WHEN excluded.listed_at >= web_conversations.listed_at THEN excluded.created_at ELSE web_conversations.created_at END,
  updated_at = CASE WHEN excluded.listed_at >= web_conversations.listed_at THEN excluded.updated_at ELSE web_conversations.updated_at END,
  archived = CASE WHEN excluded.listed_at >= web_conversations.listed_at THEN excluded.archived ELSE web_conversations.archived END,
  removed_at = CASE WHEN excluded.listed_at >= web_conversations.listed_at THEN excluded.removed_at ELSE web_conversations.removed_at END,
  listed_at = MAX(web_conversations.listed_at, excluded.listed_at),
  folder_id = CASE WHEN excluded.local_updated_at >= web_conversations.local_updated_at THEN excluded.folder_id ELSE web_conversations.folder_id END,
  tags = CASE WHEN excluded.local_updated_at >= web_conversations.local_updated_at THEN excluded.tags ELSE web_conversations.tags END,
  favorite = CASE WHEN excluded.local_updated_at >= web_conversations.local_updated_at THEN excluded.favorite ELSE web_conversations.favorite END,
  note = CASE WHEN excluded.local_updated_at >= web_conversations.local_updated_at THEN excluded.note ELSE web_conversations.note END,
  local_updated_at = MAX(web_conversations.local_updated_at, excluded.local_updated_at)";

const FOLDER_UPSERT: &str = "
INSERT INTO web_folders(id,name,created_at,local_updated_at) VALUES(?1,?2,?3,?4)
ON CONFLICT(id) DO UPDATE SET
  name = CASE WHEN excluded.local_updated_at >= web_folders.local_updated_at THEN excluded.name ELSE web_folders.name END,
  local_updated_at = MAX(web_folders.local_updated_at, excluded.local_updated_at)";

const EXCERPT_UPSERT: &str = "
INSERT INTO web_excerpts(id,site,conversation_id,url,page_title,text,note,created_at,local_updated_at)
VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
ON CONFLICT(id) DO UPDATE SET
  text = CASE WHEN excluded.local_updated_at >= web_excerpts.local_updated_at THEN excluded.text ELSE web_excerpts.text END,
  note = CASE WHEN excluded.local_updated_at >= web_excerpts.local_updated_at THEN excluded.note ELSE web_excerpts.note END,
  local_updated_at = MAX(web_excerpts.local_updated_at, excluded.local_updated_at)";

fn short_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 200
}

pub fn upsert_accounts(conn: &Connection, items: &[WebAccount]) -> Result<usize, String> {
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    let mut accepted = 0;
    for a in items
        .iter()
        .filter(|a| valid_key(&a.site, &a.remote_id, &a.key))
    {
        tx.execute(
            ACCOUNT_UPSERT,
            params![a.key, a.site, a.remote_id, a.name, a.alias, a.last_seen, a.local_updated_at],
        )
        .map_err(db_err)?;
        accepted += 1;
    }
    tx.commit().map_err(db_err)?;
    Ok(accepted)
}

pub fn upsert_conversations(conn: &Connection, items: &[WebConversation]) -> Result<usize, String> {
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    let mut accepted = 0;
    for c in items.iter().filter(|c| valid_key(&c.site, &c.id, &c.key)) {
        let tags = serde_json::to_string(&c.tags).map_err(db_err)?;
        tx.execute(
            CONVERSATION_UPSERT,
            params![
                c.key, c.site, c.account, c.id, c.title, c.created_at, c.updated_at, c.archived,
                c.removed_at, c.listed_at, c.folder_id, tags, c.favorite, c.note, c.local_updated_at
            ],
        )
        .map_err(db_err)?;
        accepted += 1;
    }
    tx.commit().map_err(db_err)?;
    Ok(accepted)
}

pub fn upsert_folders(conn: &Connection, items: &[WebFolder]) -> Result<usize, String> {
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    let mut accepted = 0;
    for f in items.iter().filter(|f| short_id(&f.id)) {
        tx.execute(FOLDER_UPSERT, params![f.id, f.name, f.created_at, f.local_updated_at])
            .map_err(db_err)?;
        accepted += 1;
    }
    tx.commit().map_err(db_err)?;
    Ok(accepted)
}

pub fn upsert_excerpts(conn: &Connection, items: &[WebExcerpt]) -> Result<usize, String> {
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    let mut accepted = 0;
    for e in items
        .iter()
        .filter(|e| short_id(&e.id) && is_site(&e.site) && e.text.len() <= 100_000)
    {
        tx.execute(
            EXCERPT_UPSERT,
            params![
                e.id, e.site, e.conversation_id, e.url, e.page_title, e.text, e.note,
                e.created_at, e.local_updated_at
            ],
        )
        .map_err(db_err)?;
        accepted += 1;
    }
    tx.commit().map_err(db_err)?;
    Ok(accepted)
}

/// Folder and excerpt deletions become tombstones, so a later sync of an old copy cannot revive them.
pub fn remove_records(conn: &Connection, items: &[Removal]) -> Result<usize, String> {
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    let mut accepted = 0;
    for r in items {
        let sql = match r.kind.as_str() {
            "folder" => "UPDATE web_folders SET deleted_at=?2 WHERE id=?1 AND deleted_at IS NULL",
            "excerpt" => "UPDATE web_excerpts SET deleted_at=?2 WHERE id=?1 AND deleted_at IS NULL",
            _ => continue,
        };
        tx.execute(sql, params![r.key, r.at]).map_err(db_err)?;
        accepted += 1;
    }
    tx.commit().map_err(db_err)?;
    Ok(accepted)
}

pub(crate) fn parse_tags(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

pub fn accounts(conn: &Connection) -> Result<Vec<WebAccount>, String> {
    let mut stmt = conn
        .prepare("SELECT key,site,remote_id,name,alias,last_seen,local_updated_at FROM web_accounts ORDER BY key")
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(WebAccount {
                key: r.get(0)?,
                site: r.get(1)?,
                remote_id: r.get(2)?,
                name: r.get(3)?,
                alias: r.get(4)?,
                last_seen: r.get(5)?,
                local_updated_at: r.get(6)?,
            })
        })
        .map_err(db_err)?;
    let out: Result<Vec<_>, _> = rows.collect();
    out.map_err(db_err)
}

pub fn conversations(conn: &Connection) -> Result<Vec<WebConversation>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT key,site,account,id,title,created_at,updated_at,archived,removed_at,listed_at,
                    folder_id,tags,favorite,note,local_updated_at
             FROM web_conversations ORDER BY key",
        )
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(WebConversation {
                key: r.get(0)?,
                site: r.get(1)?,
                account: r.get(2)?,
                id: r.get(3)?,
                title: r.get(4)?,
                created_at: r.get(5)?,
                updated_at: r.get(6)?,
                archived: r.get(7)?,
                removed_at: r.get(8)?,
                listed_at: r.get(9)?,
                folder_id: r.get(10)?,
                tags: parse_tags(&r.get::<_, String>(11)?),
                favorite: r.get(12)?,
                note: r.get(13)?,
                local_updated_at: r.get(14)?,
            })
        })
        .map_err(db_err)?;
    let out: Result<Vec<_>, _> = rows.collect();
    out.map_err(db_err)
}

pub fn folders(conn: &Connection) -> Result<Vec<WebFolder>, String> {
    let mut stmt = conn
        .prepare("SELECT id,name,created_at,local_updated_at FROM web_folders WHERE deleted_at IS NULL ORDER BY id")
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(WebFolder {
                id: r.get(0)?,
                name: r.get(1)?,
                created_at: r.get(2)?,
                local_updated_at: r.get(3)?,
            })
        })
        .map_err(db_err)?;
    let out: Result<Vec<_>, _> = rows.collect();
    out.map_err(db_err)
}

pub fn excerpts(conn: &Connection) -> Result<Vec<WebExcerpt>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id,site,conversation_id,url,page_title,text,note,created_at,local_updated_at
             FROM web_excerpts WHERE deleted_at IS NULL ORDER BY id",
        )
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(WebExcerpt {
                id: r.get(0)?,
                site: r.get(1)?,
                conversation_id: r.get(2)?,
                url: r.get(3)?,
                page_title: r.get(4)?,
                text: r.get(5)?,
                note: r.get(6)?,
                created_at: r.get(7)?,
                local_updated_at: r.get(8)?,
            })
        })
        .map_err(db_err)?;
    let out: Result<Vec<_>, _> = rows.collect();
    out.map_err(db_err)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub accounts: i64,
    pub conversations: i64,
    pub bodies: i64,
    pub folders: i64,
    pub excerpts: i64,
}

pub fn counts(conn: &Connection) -> Result<Counts, String> {
    conn.query_row(
        "SELECT (SELECT count(*) FROM web_accounts),
                (SELECT count(*) FROM web_conversations),
                (SELECT count(*) FROM web_conversations WHERE body_fetched_at IS NOT NULL),
                (SELECT count(*) FROM web_folders WHERE deleted_at IS NULL),
                (SELECT count(*) FROM web_excerpts WHERE deleted_at IS NULL)",
        [],
        |r| {
            Ok(Counts {
                accounts: r.get(0)?,
                conversations: r.get(1)?,
                bodies: r.get(2)?,
                folders: r.get(3)?,
                excerpts: r.get(4)?,
            })
        },
    )
    .map_err(db_err)
}
```

注意：账号的 `valid_key(&a.site, &a.remote_id, &a.key)` 复用了「`<site>:<id>`」格式校验（账号主键是 `<site>:<remoteId>`）。

- [ ] **Step 4: 运行，确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib webchat::store`
Expected: PASS（5 个测试）。`cargo fmt`、clippy。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/webchat/store.rs src-tauri/src/webchat/mod.rs
git commit -m "feat(webchat): webchat.sqlite3 with the sync merge rule" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 4: gzip 正文与分段组装

**Files:**
- Create: `src-tauri/src/webchat/bodies.rs`
- Modify: `src-tauri/Cargo.toml`（`flate2`）、`src-tauri/src/webchat/mod.rs`（`pub mod bodies;`）、`src-tauri/src/webchat/store.rs`（`put_body_chunk`）

**Interfaces:**
- Consumes: Task 2 `BodyChunk`、`StoredBody`、`WebMessage`、`valid_key`；Task 3 `open`、`counts`、`conversations`。
- Produces:
  - `webchat::bodies::component(raw: &str) -> String`；`body_path(root: &Path, site: &str, account: &str, id: &str) -> PathBuf`（`account` 是账号主键 `<site>:<remoteId>`，路径里只用 `<remoteId>`）；`write_body(path: &Path, body: &StoredBody) -> Result<(), String>`；`read_body(path: &Path) -> Result<StoredBody, String>`（不存在时 `"E_NOT_FOUND"`）。
  - `webchat::store::MAX_CHUNKS: usize = 64`；`put_body_chunk(conn: &Connection, root: &Path, chunk: &BodyChunk) -> Result<bool, String>`（写出正文文件时返回 `true`；分段未收齐或读取较旧时 `false`；分段编号无效 `"E_REQUEST"`）。

- [ ] **Step 1: 加依赖**

`src-tauri/Cargo.toml` 的 `[dependencies]` 里 `zip = "2"` 下一行加：

```toml
flate2 = "1"
```

（`Cargo.lock` 里已有 flate2 1.1.x，由 zip 引入；不需要新下载。）

- [ ] **Step 2: 写失败的测试**

`bodies.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_components_are_safe() {
        assert_eq!(component(".."), "_");
        assert_eq!(component(""), "_");
        assert_eq!(component("."), "_");
        assert_eq!(component("a/b\\c:d"), "a_b_c_d");
        assert_eq!(component("CON"), "_CON");
        assert_eq!(component("con.txt"), "_con.txt");
        assert_eq!(component("abc-DEF_1.2"), "abc-DEF_1.2");
        assert_eq!(component(&"x".repeat(500)).len(), 120);
    }

    #[test]
    fn bodies_live_under_site_and_account_without_escaping() {
        let root = Path::new("C:/data");
        let p = body_path(root, "chatgpt", "chatgpt:u1", "abc");
        assert!(p.ends_with(Path::new("webchat/bodies/chatgpt/u1/abc.json.gz")));
        let hostile = body_path(root, "..", "chatgpt:../..", "../x");
        assert!(hostile.starts_with(root.join("webchat").join("bodies")));
        assert!(!hostile.components().any(|c| c == std::path::Component::ParentDir));
    }

    #[test]
    fn a_body_round_trips_through_gzip() {
        let dir = tempfile::tempdir().unwrap();
        let path = body_path(dir.path(), "claude", "claude:o1", "c1");
        let body = StoredBody {
            key: "claude:c1".into(),
            site: "claude".into(),
            account: "claude:o1".into(),
            id: "c1".into(),
            title: "Rust".into(),
            updated_at: 5,
            fetched_at: 6,
            messages: vec![super::super::protocol::WebMessage {
                role: "user".into(),
                text: "hello".repeat(1000),
                at: None,
                attachments: vec![],
            }],
        };
        write_body(&path, &body).unwrap();
        let raw = std::fs::read(&path).unwrap();
        assert_eq!(&raw[..2], &[0x1f, 0x8b], "gzip magic");
        assert!(raw.len() < 1000, "repetitive text compresses");
        assert_eq!(read_body(&path).unwrap(), body);
        assert_eq!(read_body(&dir.path().join("nope.json.gz")).unwrap_err(), "E_NOT_FOUND");
    }
}
```

`store.rs` 测试模块里追加：

```rust
    fn chunk(chunk: usize, chunks: usize, fetched_at: i64, text: &str) -> BodyChunk {
        BodyChunk {
            key: "chatgpt:a".into(),
            site: "chatgpt".into(),
            account: "chatgpt:u1".into(),
            id: "a".into(),
            title: "A".into(),
            updated_at: 7,
            fetched_at,
            chunk,
            chunks,
            messages: vec![WebMessage {
                role: "user".into(),
                text: text.into(),
                at: None,
                attachments: vec![],
            }],
        }
    }

    fn body_texts(root: &Path) -> Vec<String> {
        let path = super::super::bodies::body_path(root, "chatgpt", "chatgpt:u1", "a");
        super::super::bodies::read_body(&path)
            .unwrap()
            .messages
            .into_iter()
            .map(|m| m.text)
            .collect()
    }

    #[test]
    fn a_single_chunk_is_stored_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        assert!(put_body_chunk(&conn, dir.path(), &chunk(0, 1, 100, "hello")).unwrap());
        assert_eq!(body_texts(dir.path()), vec!["hello"]);
        assert_eq!(counts(&conn).unwrap().bodies, 1);
    }

    #[test]
    fn chunks_are_assembled_in_order_whatever_order_they_arrive() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        assert!(!put_body_chunk(&conn, dir.path(), &chunk(1, 2, 100, "second")).unwrap());
        assert!(put_body_chunk(&conn, dir.path(), &chunk(0, 2, 100, "first")).unwrap());
        assert_eq!(body_texts(dir.path()), vec!["first", "second"]);
        let parts: i64 = conn
            .query_row("SELECT count(*) FROM web_body_parts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(parts, 0, "staged parts are cleared once the body is written");
    }

    #[test]
    fn an_older_read_never_replaces_a_newer_body() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        put_body_chunk(&conn, dir.path(), &chunk(0, 1, 200, "new")).unwrap();
        assert!(!put_body_chunk(&conn, dir.path(), &chunk(0, 1, 100, "old")).unwrap());
        assert_eq!(body_texts(dir.path()), vec!["new"]);
    }

    #[test]
    fn a_body_before_its_listing_creates_a_row_the_listing_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        put_body_chunk(&conn, dir.path(), &chunk(0, 1, 100, "x")).unwrap();
        upsert_conversations(&conn, &[conv("a", 50, 0)]).unwrap();
        let saved = conversations(&conn).unwrap().remove(0);
        assert_eq!(saved.title, "Title a");
        assert_eq!(counts(&conn).unwrap().bodies, 1);
    }

    #[test]
    fn bad_chunk_numbers_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        for bad in [chunk(2, 2, 1, "x"), chunk(0, 0, 1, "x"), chunk(0, MAX_CHUNKS + 1, 1, "x")] {
            assert_eq!(put_body_chunk(&conn, dir.path(), &bad).unwrap_err(), "E_REQUEST");
        }
        let mut wrong_key = chunk(0, 1, 1, "x");
        wrong_key.key = "chatgpt:b".into();
        assert_eq!(put_body_chunk(&conn, dir.path(), &wrong_key).unwrap_err(), "E_REQUEST");
    }
```

- [ ] **Step 3: 运行，确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib webchat::`
Expected: FAIL（`bodies`、`put_body_chunk` 未定义）。

- [ ] **Step 4: 实现**

`src-tauri/src/webchat/mod.rs`：加 `pub mod bodies;`。

`src-tauri/src/webchat/bodies.rs`（测试模块之前）：

```rust
//! Conversation bodies as gzip JSON: `<root>/webchat/bodies/<site>/<account>/<id>.json.gz`.
use super::protocol::StoredBody;
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// One safe path component: ASCII letters, digits, `.`, `_`, `-`; never empty, `.`, `..` or a device name.
pub fn component(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .take(120)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_end_matches('.');
    if trimmed.is_empty() {
        return "_".into();
    }
    let stem = trimmed.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        format!("_{trimmed}")
    } else {
        trimmed.to_string()
    }
}

pub fn body_path(root: &Path, site: &str, account: &str, id: &str) -> PathBuf {
    let remote = account.split_once(':').map_or(account, |(_, r)| r);
    root.join("webchat")
        .join("bodies")
        .join(component(site))
        .join(component(remote))
        .join(format!("{}.json.gz", component(id)))
}

/// Writes to a temporary file first so a crash never leaves half a body.
pub fn write_body(path: &Path, body: &StoredBody) -> Result<(), String> {
    let storage = |_| "E_STORAGE".to_string();
    let dir = path.parent().ok_or("E_STORAGE")?;
    std::fs::create_dir_all(dir).map_err(storage)?;
    let json = serde_json::to_vec(body).map_err(|_| "E_STORAGE".to_string())?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&json).map_err(storage)?;
    let bytes = encoder.finish().map_err(storage)?;
    let tmp = path.with_extension("gz.tmp");
    std::fs::write(&tmp, bytes).map_err(storage)?;
    std::fs::rename(&tmp, path).map_err(storage)
}

pub fn read_body(path: &Path) -> Result<StoredBody, String> {
    let file = std::fs::File::open(path).map_err(|_| "E_NOT_FOUND".to_string())?;
    let mut json = Vec::new();
    GzDecoder::new(file)
        .read_to_end(&mut json)
        .map_err(|_| "E_STORAGE".to_string())?;
    serde_json::from_slice(&json).map_err(|_| "E_STORAGE".to_string())
}
```

（`let storage = |_| …` 的参数类型由各处 `map_err` 推断为 `std::io::Error`；若编译器提示闭包类型不一致，改成 `fn storage(_: std::io::Error) -> String { "E_STORAGE".into() }` 的内部函数。）

`src-tauri/src/webchat/store.rs`：在 `counts` 之后追加：

```rust
/// Most pieces one body may be split into (about 57 MB of messages).
pub const MAX_CHUNKS: usize = 64;

const BODY_UPSERT: &str = "
INSERT INTO web_conversations(key,site,account,id,title,updated_at,body_fetched_at,body_updated_at,body_messages)
VALUES(?1,?2,?3,?4,?5,?6,?7,?6,?8)
ON CONFLICT(key) DO UPDATE SET body_fetched_at=excluded.body_fetched_at,
  body_updated_at=excluded.body_updated_at, body_messages=excluded.body_messages";

/// Stores one piece of a body read; once every piece of that read is in, writes the body file.
/// Returns whether the file was written. An older read never replaces a newer one.
pub fn put_body_chunk(conn: &Connection, root: &Path, chunk: &BodyChunk) -> Result<bool, String> {
    if !valid_key(&chunk.site, &chunk.id, &chunk.key)
        || chunk.chunks == 0
        || chunk.chunks > MAX_CHUNKS
        || chunk.chunk >= chunk.chunks
    {
        return Err("E_REQUEST".into());
    }
    let stored: Option<i64> = conn
        .query_row(
            "SELECT body_fetched_at FROM web_conversations WHERE key=?1",
            [&chunk.key],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_err)?
        .flatten();
    if stored.is_some_and(|at| at > chunk.fetched_at) {
        return Ok(false);
    }
    let messages = if chunk.chunks == 1 {
        chunk.messages.clone()
    } else {
        match collect_parts(conn, chunk)? {
            Some(all) => all,
            None => return Ok(false),
        }
    };
    let count = messages.len() as i64;
    let body = StoredBody {
        key: chunk.key.clone(),
        site: chunk.site.clone(),
        account: chunk.account.clone(),
        id: chunk.id.clone(),
        title: chunk.title.clone(),
        updated_at: chunk.updated_at,
        fetched_at: chunk.fetched_at,
        messages,
    };
    let path = super::bodies::body_path(root, &chunk.site, &chunk.account, &chunk.id);
    super::bodies::write_body(&path, &body)?;
    conn.execute(
        BODY_UPSERT,
        params![
            chunk.key, chunk.site, chunk.account, chunk.id, chunk.title, chunk.updated_at,
            chunk.fetched_at, count
        ],
    )
    .map_err(db_err)?;
    Ok(true)
}

/// Keeps a piece until its siblings arrive; returns every message in order once complete.
fn collect_parts(conn: &Connection, chunk: &BodyChunk) -> Result<Option<Vec<WebMessage>>, String> {
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    tx.execute(
        "DELETE FROM web_body_parts WHERE key=?1 AND fetched_at<>?2",
        params![chunk.key, chunk.fetched_at],
    )
    .map_err(db_err)?;
    let json = serde_json::to_string(&chunk.messages).map_err(db_err)?;
    tx.execute(
        "INSERT OR REPLACE INTO web_body_parts(key,fetched_at,chunk,chunks,messages) VALUES(?1,?2,?3,?4,?5)",
        params![chunk.key, chunk.fetched_at, chunk.chunk as i64, chunk.chunks as i64, json],
    )
    .map_err(db_err)?;
    let parts: Vec<String> = {
        let mut stmt = tx
            .prepare("SELECT messages FROM web_body_parts WHERE key=?1 AND fetched_at=?2 ORDER BY chunk")
            .map_err(db_err)?;
        let rows = stmt
            .query_map(params![chunk.key, chunk.fetched_at], |r| r.get::<_, String>(0))
            .map_err(db_err)?;
        let parts: Result<Vec<String>, _> = rows.collect();
        parts.map_err(db_err)?
    };
    if parts.len() < chunk.chunks {
        tx.commit().map_err(db_err)?;
        return Ok(None);
    }
    let mut all = Vec::new();
    for part in parts {
        all.extend(serde_json::from_str::<Vec<WebMessage>>(&part).map_err(db_err)?);
    }
    tx.execute("DELETE FROM web_body_parts WHERE key=?1", [&chunk.key])
        .map_err(db_err)?;
    tx.commit().map_err(db_err)?;
    Ok(Some(all))
}
```

- [ ] **Step 5: 运行，确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib webchat::`
Expected: PASS。`cargo fmt`、clippy。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/webchat/bodies.rs src-tauri/src/webchat/store.rs src-tauri/src/webchat/mod.rs
git commit -m "feat(webchat): gzip bodies assembled from chunks" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---
### Task 5: saveExport 路径校验与写入

**Files:**
- Create: `src-tauri/src/webchat/export.rs`
- Modify: `src-tauri/src/webchat/mod.rs`（`pub mod export;`）、`src-tauri/src/sessions/export.rs`（`fn safe` → `pub(crate) fn safe`）

**Interfaces:**
- Consumes: `crate::sessions::export::safe(name: &str, limit: usize) -> String`（替换 `\/:*?"<>|` 与控制字符为 `_`，去掉尾部点和空格，空时返回 `"session"`）。
- Produces（`webchat::export`）：
  - `safe_parts(rel: &str) -> Result<Vec<String>, String>`（无效 `"E_PATH"`）
  - `Saved { path: String /* 相对 <exports>/web，用 / 分隔 */, full_path: PathBuf }`
  - `save_export(exports: &Path, rel: &str, text: &str, append: bool, written: &mut HashSet<PathBuf>) -> Result<Saved, String>`

- [ ] **Step 1: 写失败的测试**（`webchat/export.rs` 末尾）

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_and_odd_paths_are_refused() {
        for bad in [
            "", "a.md/", "/abs.md", "../a.md", "a/../../b.md", "./a.md", "C:\\x.md", "C:x.md",
            "a/b/c/d/e.md", "run.exe", "notes.txt", "a\\..\\b.md",
        ] {
            assert_eq!(safe_parts(bad).unwrap_err(), "E_PATH", "{bad}");
        }
        assert_eq!(
            safe_parts("chatgpt\\2026-09-19 a*b? id.md").unwrap(),
            vec!["chatgpt".to_string(), "2026-09-19 a_b_ id.md".to_string()]
        );
    }

    #[test]
    fn writes_under_the_web_folder_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let exports = dir.path().join("exports");
        let mut written = HashSet::new();
        let first = save_export(&exports, "chatgpt/a.md", "one", false, &mut written).unwrap();
        let second = save_export(&exports, "chatgpt/a.md", "two", false, &mut written).unwrap();
        assert_eq!(first.path, "chatgpt/a.md");
        assert_eq!(second.path, "chatgpt/a (1).md");
        assert!(first.full_path.starts_with(exports.join("web")));
        assert_eq!(std::fs::read_to_string(&first.full_path).unwrap(), "one");
        assert_eq!(std::fs::read_to_string(&second.full_path).unwrap(), "two");
    }

    #[test]
    fn appends_only_to_files_this_bridge_created() {
        let dir = tempfile::tempdir().unwrap();
        let exports = dir.path().join("exports");
        let mut written = HashSet::new();
        let saved = save_export(&exports, "claude/long.md", "part 1,", false, &mut written).unwrap();
        save_export(&exports, &saved.path, " part 2", true, &mut written).unwrap();
        assert_eq!(std::fs::read_to_string(&saved.full_path).unwrap(), "part 1, part 2");
        std::fs::create_dir_all(exports.join("web").join("claude")).unwrap();
        std::fs::write(exports.join("web").join("claude").join("other.md"), "keep").unwrap();
        let err = save_export(&exports, "claude/other.md", "x", true, &mut written).unwrap_err();
        assert_eq!(err, "E_PATH");
        let other = std::fs::read_to_string(exports.join("web").join("claude").join("other.md")).unwrap();
        assert_eq!(other, "keep");
    }
}
```

- [ ] **Step 2: 运行，确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib webchat::export`
Expected: FAIL（未定义）。

- [ ] **Step 3: 实现**

`src-tauri/src/sessions/export.rs` 第 8 行：`fn safe(name: &str, limit: usize) -> String {` 改为 `pub(crate) fn safe(name: &str, limit: usize) -> String {`。

`src-tauri/src/webchat/mod.rs`：加 `pub mod export;`。

`src-tauri/src/webchat/export.rs`（测试模块之前）：

```rust
//! `saveExport`: the extension's Markdown / JSON exports, written under `<exports>/web`.
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

/// `<site>/<file>` today; a little room for later layouts, never deep trees.
const MAX_PARTS: usize = 4;

fn has_export_extension(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".json")
}

/// Checks a relative path from the extension and returns its cleaned components.
pub fn safe_parts(rel: &str) -> Result<Vec<String>, String> {
    let parts: Vec<&str> = rel.split(['/', '\\']).collect();
    if parts.len() > MAX_PARTS
        || parts
            .iter()
            .any(|p| p.trim().is_empty() || *p == "." || *p == ".." || p.contains(':'))
    {
        return Err("E_PATH".into());
    }
    let cleaned: Vec<String> = parts
        .iter()
        .map(|p| crate::sessions::export::safe(p, 120))
        .collect();
    match cleaned.last() {
        Some(name) if has_export_extension(name) => Ok(cleaned),
        _ => Err("E_PATH".into()),
    }
}

pub struct Saved {
    /// Relative to `<exports>/web`, `/`-separated; the extension appends to this path.
    pub path: String,
    pub full_path: PathBuf,
}

/// `name`, or `name (1)`, `name (2)`… when taken, like the browser's download folder.
fn create_unused(dir: &Path, name: &str) -> Result<(PathBuf, std::fs::File), String> {
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    for n in 0..1000 {
        let candidate = match (n, ext.is_empty()) {
            (0, _) => name.to_string(),
            (_, true) => format!("{stem} ({n})"),
            (_, false) => format!("{stem} ({n}).{ext}"),
        };
        let path = dir.join(candidate);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err("E_STORAGE".into()),
        }
    }
    Err("E_STORAGE".into())
}

pub fn save_export(
    exports: &Path,
    rel: &str,
    text: &str,
    append: bool,
    written: &mut HashSet<PathBuf>,
) -> Result<Saved, String> {
    let parts = safe_parts(rel)?;
    let base = exports.join("web");
    let (name, dirs) = parts.split_last().ok_or("E_PATH")?;
    let dir = dirs.iter().fold(base.clone(), |d, p| d.join(p));
    std::fs::create_dir_all(&dir).map_err(|_| "E_STORAGE".to_string())?;
    let (full_path, mut file) = if append {
        let path = dir.join(name);
        if !written.contains(&path) {
            return Err("E_PATH".into());
        }
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|_| "E_STORAGE".to_string())?;
        (path, file)
    } else {
        create_unused(&dir, name)?
    };
    file.write_all(text.as_bytes())
        .map_err(|_| "E_STORAGE".to_string())?;
    written.insert(full_path.clone());
    let path = full_path
        .strip_prefix(&base)
        .map_err(|_| "E_PATH".to_string())?
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    Ok(Saved { path, full_path })
}
```

说明：`"a.md/"` 的最后一段为空被拒；`"C:x.md"` 含冒号被拒；`"notes.txt"` 扩展名不符被拒。

- [ ] **Step 4: 运行，确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib webchat::export sessions::export`
Expected: PASS。`cargo fmt`、clippy。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/webchat/export.rs src-tauri/src/webchat/mod.rs src-tauri/src/sessions/export.rs
git commit -m "feat(webchat): saveExport under the export folder without traversal" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 6: 桥接循环、分发与 pullBackup 分页

**Files:**
- Create: `src-tauri/src/webchat/bridge.rs`
- Modify: `src-tauri/src/webchat/mod.rs`（`pub mod bridge;`）、`src-tauri/src/webchat/store.rs`（`pull`、`page_by_bytes`）

**Interfaces:**
- Consumes: Task 1 `framing`、`allowed_origin`、`root`、`export_dir`、`now_ms`；Task 2 全部协议类型；Task 3/4 `store::*`；Task 5 `export::save_export`。
- Produces:
  - `webchat::store::PULL_BUDGET: usize = 800_000`；`PullPage { items: Vec<Value>, next: Option<usize> }`（Serialize）；`page_by_bytes(all: Vec<Value>, offset: usize, budget: usize) -> PullPage`；`pull(conn: &Connection, section: &str, offset: usize, budget: usize) -> Result<PullPage, String>`（未知分区 `"E_REQUEST"`）。
  - `webchat::bridge::origin_arg(args: &[String]) -> Option<String>`；`run_stdio(origin: &str) -> i32`；`serve(input: &mut impl Read, output: &mut impl Write, ctx: &mut Context) -> i32`；`encode_response(response: &Response) -> Vec<u8>`；`Context::open(root: PathBuf, exports: PathBuf) -> Result<Context, String>`；`Context::handle(&mut self, req: Request) -> Response`。
  - 结果形状：`hello` → `{ app: "stacker", version, protocol, counts }`；`status` → `{ counts, lastSyncAt, exportDir }`；`sync*`/`removeRecords` → `{ accepted }`；`syncBody` → `{ stored }`；`pullBackup` → `{ items, next }`；`saveExport` → `{ path, fullPath }`。错误码：`E_REQUEST`、`E_UNKNOWN_TYPE`、`E_TOO_LARGE`、`E_PATH`、`E_STORAGE`。

- [ ] **Step 1: 写失败的测试**

`bridge.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn frame(value: &Value) -> Vec<u8> {
        let body = serde_json::to_vec(value).unwrap();
        let mut out = (body.len() as u32).to_le_bytes().to_vec();
        out.extend(body);
        out
    }

    fn replies(output: Vec<u8>) -> Vec<Value> {
        let mut reader = Cursor::new(output);
        let mut out = Vec::new();
        while let Some(raw) = framing::read_message(&mut reader).unwrap() {
            out.push(serde_json::from_slice(&raw).unwrap());
        }
        out
    }

    fn exchange(ctx: &mut Context, messages: &[Value]) -> Vec<Value> {
        let input: Vec<u8> = messages.iter().flat_map(frame).collect();
        let mut output = Vec::new();
        assert_eq!(serve(&mut Cursor::new(input), &mut output, ctx), 0);
        replies(output)
    }

    fn context(dir: &tempfile::TempDir) -> Context {
        Context::open(dir.path().to_path_buf(), dir.path().join("exports")).unwrap()
    }

    #[test]
    fn only_a_chrome_extension_origin_starts_bridge_mode() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            origin_arg(&args(&["stacker.exe", "chrome-extension://abc/", "--parent-window=0"])),
            Some("chrome-extension://abc/".to_string())
        );
        assert_eq!(origin_arg(&args(&["stacker.exe"])), None);
        assert_eq!(origin_arg(&args(&["stacker.exe", "--autostart"])), None);
    }

    #[test]
    fn a_foreign_origin_is_refused_before_reading_anything() {
        assert_eq!(run_stdio("chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"), 2);
    }

    #[test]
    fn hello_and_errors_keep_the_request_id() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = context(&dir);
        let out = exchange(
            &mut ctx,
            &[
                json!({"id": "1", "type": "hello", "payload": {"version": "test"}}),
                json!({"id": "2", "type": "nope"}),
                json!({"id": "3", "type": "syncAccounts", "payload": {"items": "bad"}}),
            ],
        );
        assert_eq!(out[0]["ok"], true);
        assert_eq!(out[0]["result"]["app"], "stacker");
        assert_eq!(out[0]["result"]["protocol"], PROTOCOL_VERSION);
        assert_eq!(out[1], json!({"id": "2", "ok": false, "error": "E_UNKNOWN_TYPE"}));
        assert_eq!(out[2]["error"], "E_REQUEST");
        assert!(store::meta(&ctx.conn, "last_hello_at").is_some());
    }

    #[test]
    fn a_malformed_message_gets_an_error_and_the_loop_goes_on() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = context(&dir);
        let bad = b"not json";
        let mut input = (bad.len() as u32).to_le_bytes().to_vec();
        input.extend_from_slice(bad);
        input.extend(frame(&json!({"id": "9", "type": "status"})));
        let mut output = Vec::new();
        assert_eq!(serve(&mut Cursor::new(input), &mut output, &mut ctx), 0);
        let out = replies(output);
        assert_eq!(out[0], json!({"id": "", "ok": false, "error": "E_REQUEST"}));
        assert_eq!((out[1]["id"].as_str(), out[1]["ok"].as_bool()), (Some("9"), Some(true)));
    }

    #[test]
    fn an_oversized_frame_ends_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = context(&dir);
        let input = ((MAX_MESSAGE + 1) as u32).to_le_bytes().to_vec();
        let mut output = Vec::new();
        assert_eq!(serve(&mut Cursor::new(input), &mut output, &mut ctx), 1);
        assert!(output.is_empty());
    }

    #[test]
    fn responses_over_the_limit_become_an_error() {
        let big = Response::success("7".into(), json!("x".repeat(MAX_MESSAGE)));
        let value: Value = serde_json::from_slice(&encode_response(&big)).unwrap();
        assert_eq!(value, json!({"id": "7", "ok": false, "error": "E_TOO_LARGE"}));
    }

    #[test]
    fn batches_over_the_limit_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = context(&dir);
        let items: Vec<Value> = (0..=MAX_BATCH)
            .map(|i| json!({"key": format!("chatgpt:u{i}"), "site": "chatgpt", "remoteId": format!("u{i}")}))
            .collect();
        let out = exchange(&mut ctx, &[json!({"id": "1", "type": "syncAccounts", "payload": {"items": items}})]);
        assert_eq!(out[0]["error"], "E_REQUEST");
    }

    #[test]
    fn sync_then_pull_round_trips_everything() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = context(&dir);
        let conversation = json!({
            "key": "chatgpt:a", "site": "chatgpt", "account": "chatgpt:u1", "id": "a", "title": "Trip",
            "createdAt": 1.0, "updatedAt": 1_700_000_000_000.4_f64, "archived": false, "removedAt": null,
            "listedAt": 10, "folderId": "f1", "tags": ["travel"], "favorite": true, "note": "n",
            "localUpdatedAt": 10
        });
        let excerpt = |id: &str, text: &str| json!({
            "id": id, "site": "chatgpt", "conversationId": "a", "url": "https://chatgpt.com/c/a",
            "pageTitle": "Trip", "text": text, "note": "", "createdAt": 1, "localUpdatedAt": 1
        });
        let out = exchange(
            &mut ctx,
            &[
                json!({"id": "1", "type": "syncAccounts", "payload": {"items": [{
                    "key": "chatgpt:u1", "site": "chatgpt", "remoteId": "u1", "name": "Ada",
                    "alias": "Work", "lastSeen": 10, "localUpdatedAt": 10}]}}),
                json!({"id": "2", "type": "syncFolders", "payload": {"items": [{
                    "id": "f1", "name": "Trips", "createdAt": 1, "localUpdatedAt": 1}]}}),
                json!({"id": "3", "type": "syncConversations", "payload": {"items": [conversation]}}),
                json!({"id": "4", "type": "syncBody", "payload": {
                    "key": "chatgpt:a", "site": "chatgpt", "account": "chatgpt:u1", "id": "a",
                    "title": "Trip", "updatedAt": 1_700_000_000_000_i64, "fetchedAt": 20,
                    "chunk": 0, "chunks": 1,
                    "messages": [{"role": "user", "text": "Where?", "at": null, "attachments": []}]}}),
                json!({"id": "5", "type": "syncExcerpts", "payload": {"items": [excerpt("e1", "tip"), excerpt("e2", "gone")]}}),
                json!({"id": "6", "type": "removeRecords", "payload": {"items": [{"kind": "excerpt", "key": "e2", "at": 5}]}}),
                json!({"id": "7", "type": "pullBackup", "payload": {"section": "conversations", "offset": 0}}),
                json!({"id": "8", "type": "pullBackup", "payload": {"section": "excerpts", "offset": 0}}),
                json!({"id": "9", "type": "status"}),
                json!({"id": "10", "type": "pullBackup", "payload": {"section": "secrets", "offset": 0}}),
            ],
        );
        assert!(out[..9].iter().all(|r| r["ok"] == true), "{out:?}");
        assert_eq!(out[3]["result"]["stored"], true);
        let pulled = &out[6]["result"]["items"][0];
        assert_eq!(pulled["updatedAt"], 1_700_000_000_000_i64);
        assert_eq!(pulled["tags"], json!(["travel"]));
        assert_eq!(out[6]["result"]["next"], Value::Null);
        assert_eq!(out[7]["result"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(
            out[8]["result"]["counts"],
            json!({"accounts": 1, "conversations": 1, "bodies": 1, "folders": 1, "excerpts": 1})
        );
        assert_eq!(out[9]["error"], "E_REQUEST");
        assert!(store::meta(&ctx.conn, "last_sync_at").is_some());
    }

    #[test]
    fn save_export_stays_inside_the_export_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = context(&dir);
        let out = exchange(
            &mut ctx,
            &[
                json!({"id": "1", "type": "saveExport", "payload": {"path": "chatgpt/a.md", "text": "# A"}}),
                json!({"id": "2", "type": "saveExport", "payload": {"path": "../escape.md", "text": "x"}}),
            ],
        );
        assert_eq!(out[0]["result"]["path"], "chatgpt/a.md");
        assert!(dir.path().join("exports").join("web").join("chatgpt").join("a.md").is_file());
        assert_eq!(out[1]["error"], "E_PATH");
        assert!(!dir.path().join("exports").join("escape.md").exists());
    }

    #[test]
    fn pull_pages_stay_under_the_budget() {
        let items: Vec<Value> = (0..10).map(|i| json!({"i": i, "pad": "x".repeat(100)})).collect();
        let first = store::page_by_bytes(items.clone(), 0, 300);
        assert_eq!((first.items.len(), first.next), (2, Some(2)));
        let last = store::page_by_bytes(items.clone(), 8, 300);
        assert_eq!((last.items.len(), last.next), (2, None));
        let tiny = store::page_by_bytes(items, 0, 10);
        assert_eq!(tiny.items.len(), 1, "one oversized item still moves forward");
    }
}
```

- [ ] **Step 2: 运行，确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib webchat::bridge`
Expected: FAIL（未定义）。

- [ ] **Step 3: 实现 store 分页**

`src-tauri/src/webchat/store.rs` 顶部 `use` 改为：

```rust
use super::protocol::*;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
```

在 `put_body_chunk` 之前追加：

```rust
/// Bytes of JSON per `pullBackup` page, well under the 1 MiB message limit.
pub const PULL_BUDGET: usize = 800_000;

#[derive(Debug, Serialize)]
pub struct PullPage {
    pub items: Vec<Value>,
    pub next: Option<usize>,
}

/// Items from `offset` until `budget` bytes of JSON; always at least one so paging moves on.
pub fn page_by_bytes(all: Vec<Value>, offset: usize, budget: usize) -> PullPage {
    let total = all.len();
    let mut used = 0;
    let mut items = Vec::new();
    for value in all.into_iter().skip(offset) {
        let size = value.to_string().len() + 1;
        if !items.is_empty() && used + size > budget {
            break;
        }
        used += size;
        items.push(value);
    }
    let end = offset + items.len();
    PullPage {
        next: if end < total { Some(end) } else { None },
        items,
    }
}

fn to_values<T: Serialize>(items: Vec<T>) -> Vec<Value> {
    items
        .into_iter()
        .filter_map(|i| serde_json::to_value(i).ok())
        .collect()
}

/// One page of Stacker's copy of the extension's organizing data, for 「从 Stacker 恢复」.
pub fn pull(conn: &Connection, section: &str, offset: usize, budget: usize) -> Result<PullPage, String> {
    let all = match section {
        "accounts" => to_values(accounts(conn)?),
        "folders" => to_values(folders(conn)?),
        "conversations" => to_values(conversations(conn)?),
        "excerpts" => to_values(excerpts(conn)?),
        _ => return Err("E_REQUEST".into()),
    };
    Ok(page_by_bytes(all, offset, budget))
}
```

- [ ] **Step 4: 实现桥接**

`src-tauri/src/webchat/mod.rs`：加 `pub mod bridge;`。

`src-tauri/src/webchat/bridge.rs`（测试模块之前）：

```rust
//! Bridge mode: Chrome / Edge start `stacker.exe chrome-extension://<id>/ --parent-window=…`
//! and talk to it over stdin / stdout. No window, no Tauri.
use super::framing::{self, MAX_MESSAGE};
use super::protocol::*;
use super::{export, store};
use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::PathBuf;

/// The caller origin Chrome passes as the first argument on Windows.
pub fn origin_arg(args: &[String]) -> Option<String> {
    args.get(1)
        .filter(|a| a.starts_with("chrome-extension://"))
        .cloned()
}

/// Exit codes: 0 the browser closed the pipe, 1 framing or I/O error, 2 wrong caller, 3 no storage.
pub fn run_stdio(origin: &str) -> i32 {
    if origin != super::allowed_origin() {
        return 2;
    }
    let mut ctx = match Context::open(super::root(), super::export_dir()) {
        Ok(ctx) => ctx,
        Err(_) => return 3,
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(&mut stdin.lock(), &mut stdout.lock(), &mut ctx)
}

pub fn serve(input: &mut impl Read, output: &mut impl Write, ctx: &mut Context) -> i32 {
    loop {
        let raw = match framing::read_message(input) {
            Ok(Some(raw)) => raw,
            Ok(None) => return 0,
            Err(_) => return 1,
        };
        let response = match serde_json::from_slice::<Request>(&raw) {
            Ok(request) => ctx.handle(request),
            Err(_) => Response::failure(String::new(), "E_REQUEST"),
        };
        if framing::write_message(output, &encode_response(&response)).is_err() {
            return 1;
        }
    }
}

/// A response that would not fit one native message is replaced by `E_TOO_LARGE`.
pub fn encode_response(response: &Response) -> Vec<u8> {
    let bytes = serde_json::to_vec(response).unwrap_or_default();
    if bytes.len() <= MAX_MESSAGE {
        return bytes;
    }
    serde_json::to_vec(&Response::failure(response.id.clone(), "E_TOO_LARGE")).unwrap_or_default()
}

pub struct Context {
    conn: Connection,
    root: PathBuf,
    exports: PathBuf,
    /// Export files this process created; only these may be appended to.
    written: HashSet<PathBuf>,
}

fn parse<T: DeserializeOwned>(payload: Value) -> Result<T, String> {
    serde_json::from_value(payload).map_err(|_| "E_REQUEST".to_string())
}

fn batch<T>(items: Items<T>) -> Result<Vec<T>, String> {
    if items.items.len() > MAX_BATCH {
        return Err("E_REQUEST".into());
    }
    Ok(items.items)
}

impl Context {
    pub fn open(root: PathBuf, exports: PathBuf) -> Result<Self, String> {
        Ok(Self {
            conn: store::open(&root)?,
            root,
            exports,
            written: HashSet::new(),
        })
    }

    pub fn handle(&mut self, request: Request) -> Response {
        match self.dispatch(&request.kind, request.payload) {
            Ok(result) => Response::success(request.id, result),
            Err(code) => Response::failure(request.id, &code),
        }
    }

    fn synced(&self, result: Value) -> Result<Value, String> {
        store::set_meta(&self.conn, "last_sync_at", super::now_ms())?;
        Ok(result)
    }

    fn dispatch(&mut self, kind: &str, payload: Value) -> Result<Value, String> {
        match kind {
            "hello" => {
                store::set_meta(&self.conn, "last_hello_at", super::now_ms())?;
                Ok(json!({
                    "app": "stacker",
                    "version": env!("CARGO_PKG_VERSION"),
                    "protocol": PROTOCOL_VERSION,
                    "counts": store::counts(&self.conn)?,
                }))
            }
            "status" => Ok(json!({
                "counts": store::counts(&self.conn)?,
                "lastSyncAt": store::meta(&self.conn, "last_sync_at"),
                "exportDir": self.exports.join("web").to_string_lossy(),
            })),
            "syncAccounts" => {
                let items: Items<WebAccount> = parse(payload)?;
                let n = store::upsert_accounts(&self.conn, &batch(items)?)?;
                self.synced(json!({ "accepted": n }))
            }
            "syncConversations" => {
                let items: Items<WebConversation> = parse(payload)?;
                let n = store::upsert_conversations(&self.conn, &batch(items)?)?;
                self.synced(json!({ "accepted": n }))
            }
            "syncFolders" => {
                let items: Items<WebFolder> = parse(payload)?;
                let n = store::upsert_folders(&self.conn, &batch(items)?)?;
                self.synced(json!({ "accepted": n }))
            }
            "syncExcerpts" => {
                let items: Items<WebExcerpt> = parse(payload)?;
                let n = store::upsert_excerpts(&self.conn, &batch(items)?)?;
                self.synced(json!({ "accepted": n }))
            }
            "removeRecords" => {
                let items: Items<Removal> = parse(payload)?;
                let n = store::remove_records(&self.conn, &batch(items)?)?;
                self.synced(json!({ "accepted": n }))
            }
            "syncBody" => {
                let chunk: BodyChunk = parse(payload)?;
                let stored = store::put_body_chunk(&self.conn, &self.root, &chunk)?;
                self.synced(json!({ "stored": stored }))
            }
            "pullBackup" => {
                let request: PullRequest = parse(payload)?;
                let page = store::pull(&self.conn, &request.section, request.offset, store::PULL_BUDGET)?;
                serde_json::to_value(page).map_err(|_| "E_STORAGE".to_string())
            }
            "saveExport" => {
                let request: SaveExport = parse(payload)?;
                let saved = export::save_export(
                    &self.exports,
                    &request.path,
                    &request.text,
                    request.append,
                    &mut self.written,
                )?;
                Ok(json!({ "path": saved.path, "fullPath": saved.full_path.to_string_lossy() }))
            }
            _ => Err("E_UNKNOWN_TYPE".into()),
        }
    }
}
```

- [ ] **Step 5: 运行，确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib webchat::`
Expected: PASS。`cargo fmt`、clippy。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/webchat/bridge.rs src-tauri/src/webchat/store.rs src-tauri/src/webchat/mod.rs
git commit -m "feat(webchat): bridge dispatcher with paged pullBackup" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 7: 桥接模式入口与命令行探针

**Files:**
- Modify: `src-tauri/src/lib.rs`（`run()` 开头）
- Create: `scripts/webchat-bridge-probe.mjs`

**Interfaces:**
- Consumes: `webchat::bridge::origin_arg`、`run_stdio`。
- Produces: `node scripts/webchat-bridge-probe.mjs <stacker.exe> [--write]`：像浏览器一样启动桥接模式并逐项检查；`--write` 需要 `STACKER_WEBCHAT_DIR` 指向临时目录（仅调试构建生效）。全部通过时退出码 0。Task 19 使用。

- [ ] **Step 1: 接入入口**

`src-tauri/src/lib.rs` 的 `pub fn run() {` 之后、`if let Some((file, token)) = space_analysis::elevated::helper_arg()` 之前插入：

```rust
    // 浏览器插件的本地消息桥：Chrome / Edge 以插件来源为第一个参数启动本程序。
    // 只走标准输入输出；在单实例插件和界面之前退出，因此不会打开任何窗口。
    let args: Vec<String> = std::env::args().collect();
    if let Some(origin) = webchat::bridge::origin_arg(&args) {
        std::process::exit(webchat::bridge::run_stdio(&origin));
    }
```

- [ ] **Step 2: 写探针脚本**

`scripts/webchat-bridge-probe.mjs`：

```js
// Drives Stacker's native-messaging bridge from the command line, the way Chrome does.
// Usage: node scripts/webchat-bridge-probe.mjs <path-to-stacker.exe> [--write]
// Read-only by default (hello, pullBackup, status). --write also syncs sample records and
// needs STACKER_WEBCHAT_DIR pointing at a scratch folder (honoured by debug builds only).
import { spawn, spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const [exe, flag] = process.argv.slice(2);
if (!exe || !existsSync(exe)) throw new Error("usage: node scripts/webchat-bridge-probe.mjs <stacker.exe> [--write]");
const write = flag === "--write";
if (write && !process.env.STACKER_WEBCHAT_DIR) throw new Error("--write needs STACKER_WEBCHAT_DIR set to a scratch folder");
const id = readFileSync(new URL("../extension/EXTENSION_ID", import.meta.url), "utf8").trim();

let failed = false;
const report = (name, ok, detail = "") => {
  console.log(`${ok ? "ok  " : "FAIL"} ${name}${detail ? `  ${detail}` : ""}`);
  if (!ok) failed = true;
};

const foreign = spawnSync(exe, ["chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"], { timeout: 10_000 });
report("a foreign extension is refused", foreign.status === 2, `exit ${foreign.status}`);

const frame = (obj) => {
  const body = Buffer.from(JSON.stringify(obj), "utf8");
  const head = Buffer.alloc(4);
  head.writeUInt32LE(body.length);
  return Buffer.concat([head, body]);
};

const child = spawn(exe, [`chrome-extension://${id}/`, "--parent-window=0"], { stdio: ["pipe", "pipe", "inherit"] });
const exited = new Promise((resolve) => child.on("exit", resolve));
let buffer = Buffer.alloc(0);
const waiting = new Map();
child.stdout.on("data", (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  while (buffer.length >= 4) {
    const len = buffer.readUInt32LE(0);
    if (buffer.length < 4 + len) break;
    const message = JSON.parse(buffer.subarray(4, 4 + len).toString("utf8"));
    buffer = buffer.subarray(4 + len);
    waiting.get(message.id)?.(message);
    waiting.delete(message.id);
  }
});

let seq = 0;
function call(type, payload = {}) {
  const requestId = String(++seq);
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`${type} timed out`)), 10_000);
    waiting.set(requestId, (m) => { clearTimeout(timer); resolve(m); });
    child.stdin.write(frame({ id: requestId, type, payload }));
  });
}
async function expectOk(name, type, payload) {
  const res = await call(type, payload);
  report(name, res.ok === true, JSON.stringify(res.ok ? res.result : res.error).slice(0, 160));
  return res;
}

const now = Date.now();
await expectOk("hello", "hello", { version: "probe" });
if (write) {
  const account = "chatgpt:probe";
  await expectOk("syncAccounts", "syncAccounts", { items: [{ key: account, site: "chatgpt", remoteId: "probe", name: "Probe", alias: "Work", lastSeen: now, localUpdatedAt: now }] });
  await expectOk("syncFolders", "syncFolders", { items: [{ id: "f1", name: "Trips", createdAt: now, localUpdatedAt: now }] });
  await expectOk("syncConversations", "syncConversations", { items: [{ key: "chatgpt:probe-1", site: "chatgpt", account, id: "probe-1", title: "Probe", createdAt: now, updatedAt: now, archived: false, removedAt: null, listedAt: now, folderId: "f1", tags: ["probe"], favorite: true, note: "note", localUpdatedAt: now }] });
  const head = { key: "chatgpt:probe-1", site: "chatgpt", account, id: "probe-1", title: "Probe", updatedAt: now, fetchedAt: now, chunks: 2 };
  await expectOk("syncBody 2/2 first", "syncBody", { ...head, chunk: 1, messages: [{ role: "assistant", text: "answer", at: null, attachments: [] }] });
  await expectOk("syncBody 1/2 second", "syncBody", { ...head, chunk: 0, messages: [{ role: "user", text: "question", at: null, attachments: [] }] });
  const body = join(process.env.STACKER_WEBCHAT_DIR, "webchat", "bodies", "chatgpt", "probe", "probe-1.json.gz");
  report("body file written", existsSync(body), body);
  await expectOk("syncExcerpts", "syncExcerpts", { items: [{ id: "e1", site: "chatgpt", conversationId: "probe-1", url: "https://chatgpt.com/c/probe-1", pageTitle: "Probe", text: "tip", note: "", createdAt: now, localUpdatedAt: now }] });
  await expectOk("removeRecords", "removeRecords", { items: [{ kind: "excerpt", key: "e1", at: now + 1 }] });
  const saved = await expectOk("saveExport", "saveExport", { path: "chatgpt/probe.md", text: "# Probe\n", append: false });
  if (saved.ok) await expectOk("saveExport append", "saveExport", { path: saved.result.path, text: "more\n", append: true });
  const traversal = await call("saveExport", { path: "../escape.md", text: "x", append: false });
  report("traversal refused", !traversal.ok && traversal.error === "E_PATH", traversal.error);
}
for (const section of ["accounts", "folders", "conversations", "excerpts"]) {
  await expectOk(`pullBackup ${section}`, "pullBackup", { section, offset: 0 });
}
await expectOk("status", "status");
child.stdin.end();
const code = await exited;
report("bridge exits when the pipe closes", code === 0, `exit ${code}`);
process.exitCode = failed ? 1 : 0;
```

- [ ] **Step 3: 构建并用临时目录跑探针**

Run（PowerShell）:

```powershell
cargo build --manifest-path src-tauri/Cargo.toml
$env:STACKER_WEBCHAT_DIR = Join-Path $env:TEMP "stacker-webchat-probe"
Remove-Item $env:STACKER_WEBCHAT_DIR -Recurse -Force -ErrorAction SilentlyContinue
node scripts/webchat-bridge-probe.mjs src-tauri/target/debug/stacker.exe --write
Remove-Item Env:STACKER_WEBCHAT_DIR
```

Expected: 每行 `ok`，最后 `ok   bridge exits when the pipe closes  exit 0`，脚本退出码 0；运行期间没有任何 Stacker 窗口出现（即使 Stacker 界面已在运行，也不会被单实例插件唤起）。

- [ ] **Step 4: 全量检查**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`、`cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`、`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
Expected: 全部通过。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/lib.rs scripts/webchat-bridge-probe.mjs
git commit -m "feat(webchat): bridge mode entry and a command-line probe" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 8: 主机清单、注册表登记与插件文件夹

**Files:**
- Create: `src-tauri/src/webchat/host.rs`
- Modify: `src-tauri/src/webchat/mod.rs`（`pub mod host;`）

**Interfaces:**
- Consumes: `webchat::root()`、`extension_id()`（仅实机测试用）。
- Produces（`webchat::host`）：
  - `HOST_NAME: &str = "com.stacker.webchat"`
  - `Browser { Chrome, Edge }`（serde 小写）；`Browser::ALL: [Browser; 2]`；`Browser::parse(name: &str) -> Result<Browser, String>`（`"chrome" | "edge"`，否则 `"E_REQUEST"`）；`Browser::subkey(self) -> String`
  - `trait Registry { fn read_default(&self, subkey: &str) -> Option<String>; fn write_default(&self, subkey: &str, value: &str) -> Result<(), String>; fn delete_key(&self, subkey: &str) -> Result<(), String>; }`；`UserRegistry`（HKCU，winreg）；失败码 `"E_REGISTRY"`
  - `manifest_path(root: &Path) -> PathBuf`；`manifest_json(exe: &Path, extension_id: &str) -> String`
  - `HostState { Off, Connected, Stale }`（serde snake_case）；`BrowserStatus { browser, state, registered: String }`（camelCase）
  - `status(reg: &dyn Registry, root: &Path, exe: &Path) -> Vec<BrowserStatus>`；`connect(reg: &dyn Registry, browser: Browser, root: &Path, exe: &Path, extension_id: &str) -> Result<(), String>`；`disconnect(reg: &dyn Registry, browser: Browser, root: &Path) -> Result<(), String>`
  - `pick_extension_dir(candidates: &[PathBuf]) -> Option<PathBuf>`；`extension_dir() -> (PathBuf, bool)`（找到的目录与 `true`；找不到时返回应在的位置与 `false`）

- [ ] **Step 1: 写失败的测试**（`host.rs` 末尾）

```rust
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
        status(reg, root, exe).into_iter().map(|s| s.state).collect()
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
        assert_eq!(v["allowed_origins"], serde_json::json!([format!("chrome-extension://{ID}/")]));
        assert!(manifest_path(Path::new("D:\\data")).ends_with("native-messaging\\com.stacker.webchat.json"));
    }

    #[test]
    fn connect_status_and_disconnect() {
        let dir = tempfile::tempdir().unwrap();
        let reg = FakeRegistry::default();
        let exe = dir.path().join("stacker.exe");
        assert_eq!(states(&reg, dir.path(), &exe), vec![HostState::Off, HostState::Off]);

        connect(&reg, Browser::Chrome, dir.path(), &exe, ID).unwrap();
        let manifest = manifest_path(dir.path());
        assert!(manifest.is_file());
        assert_eq!(
            reg.read_default(&Browser::Chrome.subkey()).unwrap(),
            manifest.to_string_lossy()
        );
        assert_eq!(states(&reg, dir.path(), &exe), vec![HostState::Connected, HostState::Off]);
        // Another Stacker build is running now: the registration points at the old exe.
        let other = dir.path().join("other.exe");
        assert_eq!(states(&reg, dir.path(), &other)[0], HostState::Stale);

        connect(&reg, Browser::Edge, dir.path(), &exe, ID).unwrap();
        disconnect(&reg, Browser::Chrome, dir.path()).unwrap();
        assert!(manifest.is_file(), "Edge still uses the manifest");
        assert_eq!(states(&reg, dir.path(), &exe), vec![HostState::Off, HostState::Connected]);
        disconnect(&reg, Browser::Edge, dir.path()).unwrap();
        assert!(!manifest.exists());
        disconnect(&reg, Browser::Edge, dir.path()).unwrap();
    }

    #[test]
    fn a_key_pointing_elsewhere_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let reg = FakeRegistry::default();
        reg.write_default(&Browser::Chrome.subkey(), "D:\\old\\com.stacker.webchat.json")
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
        assert_eq!(pick_extension_dir(&[installed.clone(), dev.clone()]), Some(dev));
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
        assert!(std::env::var_os("STACKER_WEBCHAT_DIR").is_none(), "unset STACKER_WEBCHAT_DIR");
        let root = crate::webchat::root();
        connect(&UserRegistry, Browser::Chrome, &root, &exe, crate::webchat::extension_id()).unwrap();
        let now = status(&UserRegistry, &root, &exe);
        println!("{now:?}");
        assert_eq!(now[0].state, HostState::Connected);
    }
}
```

- [ ] **Step 2: 运行，确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib webchat::host`
Expected: FAIL（未定义）。

- [ ] **Step 3: 实现**

`src-tauri/src/webchat/mod.rs`：加 `pub mod host;`。

`src-tauri/src/webchat/host.rs`（测试模块之前）：

```rust
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
            BrowserStatus { browser, state, registered }
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
    std::fs::write(&path, manifest_json(exe, extension_id))
        .map_err(|_| "E_STORAGE".to_string())?;
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
```

说明：`delete_subkey` 删除没有子键的键；主机键下没有子键。

- [ ] **Step 4: 运行，确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib webchat::host`
Expected: PASS（5 个，`live_register_chrome` 显示 ignored）。`cargo fmt`、clippy。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/webchat/host.rs src-tauri/src/webchat/mod.rs
git commit -m "feat(webchat): host manifest and Chrome / Edge registration" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 9: 列表、搜索、摘要与 Tauri 命令

**Files:**
- Create: `src-tauri/src/webchat/commands.rs`
- Modify: `src-tauri/src/webchat/store.rs`（读侧）、`src-tauri/src/webchat/mod.rs`（`pub mod commands;`）、`src-tauri/src/sessions/commands.rs`（`blocking`、`explorer` 改为 `pub(crate)`）、`src-tauri/src/lib.rs`（删 `allow(dead_code)`、注册命令）

**Interfaces:**
- Consumes: Task 3/4/6 `store::*`、`bodies::*`；Task 8 `host::*`；`crate::sessions::summary::{choose, summarize_text, load_settings, SummarySettings, RunnerChoice}`；`crate::sessions::summary_job::{Runner, live_runner}`；`crate::sessions::annotations::connect`；`crate::runner::CancelFlag`。
- Produces:
  - `webchat::store`：`WebQuery { site, account, search, full_text: bool, offset: usize }`（Deserialize camelCase，默认值）；`WebChatRow { key, site, account, account_name, id, title, created_at, updated_at, archived, removed_at: Option<i64>, folder: Option<String>, tags, favorite, note, body_fetched_at: Option<i64>, body_messages: i64, body_stale: bool, summary: Option<String>, summary_by, summary_at: i64, summary_stale: bool }`（Serialize camelCase）；`AccountOption { key, site, name }`；`WebPage { items: Vec<WebChatRow>, total: usize, accounts: Vec<AccountOption> }`；`WEB_PAGE_SIZE: usize = 100`；`list(conn, root: &Path, q: &WebQuery) -> Result<WebPage, String>`；`chat(conn, key: &str) -> Result<WebChatRow, String>`（`"E_NOT_FOUND"`）；`body(root: &Path, row: &WebChatRow) -> Result<StoredBody, String>`；`save_summary(conn, key, text, by, at: i64) -> Result<(), String>`
  - Tauri 命令（前端 `invoke` 名与参数）：`webchat_status() -> WebchatStatus`；`webchat_connect(browser: String) -> WebchatStatus`；`webchat_disconnect(browser: String) -> WebchatStatus`；`webchat_open(target: String /* "extension" | "exports" */) -> ()`；`webchat_list(query: WebQuery) -> WebPage`；`webchat_read(key: String) -> WebChatDetail`；`webchat_summarize(key: String, settings: Option<SummarySettings>, locale: String) -> WebChatRow`；`webchat_summary_cancel()`。
  - `WebchatStatus { extensionId, extensionDir, extensionFound, dataDir, exportDir, browsers: Vec<BrowserStatus>, lastHelloAt: Option<i64>, lastSyncAt: Option<i64>, counts: Counts }`；`WebChatDetail { chat: WebChatRow, messages: Vec<WebMessage>, chars: usize, runner: RunnerChoice }`。
  - 新错误码：`E_NO_EXTENSION`、`E_NO_BODY`、`E_SUMMARY_BUSY`、`E_REGISTRY`（Task 11 在前端补文案）。

- [ ] **Step 1: 写失败的测试**

`store.rs` 测试模块追加：

```rust
    fn seed(conn: &Connection, root: &Path) {
        let account = |key: &str, site: &str, remote: &str, name: &str, alias: &str| WebAccount {
            key: key.into(),
            site: site.into(),
            remote_id: remote.into(),
            name: name.into(),
            alias: alias.into(),
            last_seen: 1,
            local_updated_at: 1,
        };
        upsert_accounts(
            conn,
            &[
                account("chatgpt:u1", "chatgpt", "u1", "Ada", "Work"),
                account("claude:o1", "claude", "o1", "Claude", ""),
            ],
        )
        .unwrap();
        upsert_folders(conn, &[WebFolder { id: "f1".into(), name: "Trips".into(), created_at: 1, local_updated_at: 1 }])
            .unwrap();
        let mut a = conv("a", 10, 10);
        a.title = "Trip plan".into();
        a.folder_id = Some("f1".into());
        a.tags = vec!["travel".into()];
        a.updated_at = 20;
        let b = WebConversation {
            key: "claude:b".into(),
            site: "claude".into(),
            account: "claude:o1".into(),
            id: "b".into(),
            title: "Rust lifetimes".into(),
            note: "borrowck".into(),
            updated_at: 30,
            listed_at: 10,
            ..Default::default()
        };
        upsert_conversations(conn, &[a, b]).unwrap();
        let mut body = chunk(0, 1, 25, "We should visit Kyoto");
        body.updated_at = 20;
        put_body_chunk(conn, root, &body).unwrap();
    }

    fn keys(page: &WebPage) -> Vec<&str> {
        page.items.iter().map(|r| r.key.as_str()).collect()
    }

    #[test]
    fn lists_newest_first_with_names_and_filters() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        seed(&conn, dir.path());
        let all = list(&conn, dir.path(), &WebQuery::default()).unwrap();
        assert_eq!(keys(&all), vec!["claude:b", "chatgpt:a"]);
        assert_eq!(all.total, 2);
        let a = &all.items[1];
        assert_eq!((a.account_name.as_str(), a.folder.as_deref()), ("Work", Some("Trips")));
        assert_eq!(a.body_messages, 1);
        assert_eq!(all.items[0].account_name, "Claude");
        assert_eq!(all.accounts.len(), 2);
        let site = WebQuery { site: "chatgpt".into(), ..Default::default() };
        assert_eq!(keys(&list(&conn, dir.path(), &site).unwrap()), vec!["chatgpt:a"]);
        let account = WebQuery { account: "claude:o1".into(), ..Default::default() };
        assert_eq!(keys(&list(&conn, dir.path(), &account).unwrap()), vec!["claude:b"]);
    }

    #[test]
    fn search_matches_titles_notes_tags_and_bodies_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        seed(&conn, dir.path());
        let search = |text: &str, full_text: bool| {
            let q = WebQuery { search: text.into(), full_text, ..Default::default() };
            keys(&list(&conn, dir.path(), &q).unwrap()).into_iter().map(String::from).collect::<Vec<_>>()
        };
        assert_eq!(search("BORROWCK", false), vec!["claude:b"]);
        assert_eq!(search("travel", false), vec!["chatgpt:a"]);
        assert!(search("kyoto", false).is_empty());
        assert_eq!(search("kyoto", true), vec!["chatgpt:a"]);
    }

    #[test]
    fn stale_bodies_and_summaries_are_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(dir.path()).unwrap();
        seed(&conn, dir.path());
        assert!(!chat(&conn, "chatgpt:a").unwrap().body_stale);
        save_summary(&conn, "chatgpt:a", "Go to Kyoto", "claude / sonnet / low", 50).unwrap();
        let summarized = chat(&conn, "chatgpt:a").unwrap();
        assert_eq!(summarized.summary.as_deref(), Some("Go to Kyoto"));
        assert!(!summarized.summary_stale);
        let mut newer = conv("a", 40, 0);
        newer.updated_at = 40;
        upsert_conversations(&conn, &[newer]).unwrap();
        assert!(chat(&conn, "chatgpt:a").unwrap().body_stale);
        let mut fresh = chunk(0, 1, 60, "Now Osaka");
        fresh.updated_at = 40;
        put_body_chunk(&conn, dir.path(), &fresh).unwrap();
        let after = chat(&conn, "chatgpt:a").unwrap();
        assert!(!after.body_stale && after.summary_stale);
        assert_eq!(chat(&conn, "chatgpt:zzz").unwrap_err(), "E_NOT_FOUND");
    }
```

`commands.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{RunOutput, RunRequest};
    use crate::webchat::protocol::{BodyChunk, WebConversation};
    use std::sync::Arc;

    #[test]
    fn web_chats_use_claude_unless_codex_is_fixed() {
        assert_eq!(runner_for(&SummarySettings::default()).agent, Agent::Claude);
        let codex = SummarySettings { runner: "codex".into(), ..Default::default() };
        assert_eq!(runner_for(&codex).agent, Agent::Codex);
    }

    #[test]
    fn markdown_has_one_heading_per_message() {
        let messages = vec![
            WebMessage { role: "user".into(), text: "Where should we go?".into(), at: None, attachments: vec![] },
            WebMessage { role: "assistant".into(), text: "Kyoto.".into(), at: None, attachments: vec![] },
        ];
        let md = body_markdown("Trip", &messages);
        assert!(md.starts_with("# Trip\n\n### "));
        assert_eq!(md.matches("\n### ").count(), 2);
        assert!(md.contains("Where should we go?") && md.contains("Kyoto."));
    }

    #[test]
    fn summarize_saves_the_summary_with_its_runner() {
        let dir = tempfile::tempdir().unwrap();
        let conn = store::open(dir.path()).unwrap();
        let conversation = |id: &str| WebConversation {
            key: format!("chatgpt:{id}"),
            site: "chatgpt".into(),
            account: "chatgpt:u1".into(),
            id: id.into(),
            title: "Trip".into(),
            updated_at: 5,
            listed_at: 1,
            ..Default::default()
        };
        store::upsert_conversations(&conn, &[conversation("a"), conversation("nobody")]).unwrap();
        let body = BodyChunk {
            key: "chatgpt:a".into(),
            site: "chatgpt".into(),
            account: "chatgpt:u1".into(),
            id: "a".into(),
            title: "Trip".into(),
            updated_at: 5,
            fetched_at: 6,
            chunk: 0,
            chunks: 1,
            messages: vec![WebMessage { role: "user".into(), text: "Where should we go?".into(), at: None, attachments: vec![] }],
        };
        store::put_body_chunk(&conn, dir.path(), &body).unwrap();
        let run: Runner = Arc::new(|req: &RunRequest, _: &CancelFlag| {
            assert!(req.prompt.contains("Where should we go?"));
            assert_eq!(req.backend, "claude");
            Ok::<_, String>(RunOutput { text: "Visit Kyoto".into() })
        });
        let row = summarize_in(dir.path(), "chatgpt:a", &SummarySettings::default(), "en", &CancelFlag::default(), &run)
            .unwrap();
        assert_eq!(row.summary.as_deref(), Some("Visit Kyoto"));
        assert_eq!(row.summary_by, "claude / sonnet / low");
        let missing = summarize_in(dir.path(), "chatgpt:nobody", &SummarySettings::default(), "en", &CancelFlag::default(), &run);
        assert_eq!(missing.unwrap_err(), "E_NO_BODY");
    }
}
```

- [ ] **Step 2: 运行，确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib webchat::`
Expected: FAIL（未定义）。

- [ ] **Step 3: 实现 store 读侧**

`store.rs` 顶部 `use serde::Serialize;` 改为 `use serde::{Deserialize, Serialize};`。在文件测试模块之前追加：

```rust
pub const WEB_PAGE_SIZE: usize = 100;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebQuery {
    pub site: String,
    pub account: String,
    pub search: String,
    pub full_text: bool,
    pub offset: usize,
}

/// A web chat for the 网页对话 tab. Times are milliseconds (browser clocks).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebChatRow {
    pub key: String,
    pub site: String,
    pub account: String,
    pub account_name: String,
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub archived: bool,
    pub removed_at: Option<i64>,
    pub folder: Option<String>,
    pub tags: Vec<String>,
    pub favorite: bool,
    pub note: String,
    pub body_fetched_at: Option<i64>,
    pub body_messages: i64,
    /// The site changed the conversation after its body was read.
    pub body_stale: bool,
    pub summary: Option<String>,
    pub summary_by: String,
    pub summary_at: i64,
    /// A newer body arrived after the summary was written.
    pub summary_stale: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountOption {
    pub key: String,
    pub site: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebPage {
    pub items: Vec<WebChatRow>,
    pub total: usize,
    pub accounts: Vec<AccountOption>,
}

const ROW_SELECT: &str = "
SELECT c.key, c.site, c.account, COALESCE(NULLIF(a.alias,''), NULLIF(a.name,''), c.account),
       c.id, c.title, c.created_at, c.updated_at, c.archived, c.removed_at, f.name, c.tags,
       c.favorite, c.note, c.body_fetched_at, c.body_messages, c.body_updated_at,
       c.summary, c.summary_by, c.summary_at, c.summary_body_at
FROM web_conversations c
LEFT JOIN web_accounts a ON a.key = c.account
LEFT JOIN web_folders f ON f.id = c.folder_id AND f.deleted_at IS NULL";

fn chat_row(r: &rusqlite::Row) -> rusqlite::Result<WebChatRow> {
    let updated_at: i64 = r.get(7)?;
    let body_updated_at: Option<i64> = r.get(16)?;
    let summary: String = r.get(17)?;
    let summary_body_at: Option<i64> = r.get(20)?;
    Ok(WebChatRow {
        key: r.get(0)?,
        site: r.get(1)?,
        account: r.get(2)?,
        account_name: r.get(3)?,
        id: r.get(4)?,
        title: r.get(5)?,
        created_at: r.get(6)?,
        updated_at,
        archived: r.get(8)?,
        removed_at: r.get(9)?,
        folder: r.get(10)?,
        tags: parse_tags(&r.get::<_, String>(11)?),
        favorite: r.get(12)?,
        note: r.get(13)?,
        body_fetched_at: r.get(14)?,
        body_messages: r.get(15)?,
        body_stale: body_updated_at.is_some_and(|at| at < updated_at),
        summary_stale: !summary.is_empty() && summary_body_at != body_updated_at,
        summary: Some(summary).filter(|s| !s.is_empty()),
        summary_by: r.get(18)?,
        summary_at: r.get(19)?,
    })
}

fn chat_rows(conn: &Connection) -> Result<Vec<WebChatRow>, String> {
    let mut stmt = conn
        .prepare(&format!("{ROW_SELECT} ORDER BY c.updated_at DESC, c.key"))
        .map_err(db_err)?;
    let rows = stmt.query_map([], chat_row).map_err(db_err)?;
    let out: Result<Vec<_>, _> = rows.collect();
    out.map_err(db_err)
}

pub fn chat(conn: &Connection, key: &str) -> Result<WebChatRow, String> {
    conn.query_row(&format!("{ROW_SELECT} WHERE c.key=?1"), [key], chat_row)
        .optional()
        .map_err(db_err)?
        .ok_or_else(|| "E_NOT_FOUND".to_string())
}

pub fn body(root: &Path, row: &WebChatRow) -> Result<StoredBody, String> {
    super::bodies::read_body(&super::bodies::body_path(root, &row.site, &row.account, &row.id))
}

fn account_options(conn: &Connection) -> Result<Vec<AccountOption>, String> {
    let mut stmt = conn
        .prepare("SELECT key, site, COALESCE(NULLIF(alias,''), NULLIF(name,''), key) FROM web_accounts ORDER BY site, key")
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(AccountOption { key: r.get(0)?, site: r.get(1)?, name: r.get(2)? })
        })
        .map_err(db_err)?;
    let out: Result<Vec<_>, _> = rows.collect();
    out.map_err(db_err)
}

fn matches_fields(row: &WebChatRow, needle: &str) -> bool {
    row.title.to_lowercase().contains(needle)
        || row.note.to_lowercase().contains(needle)
        || row.tags.iter().any(|t| t.to_lowercase().contains(needle))
        || row
            .summary
            .as_deref()
            .is_some_and(|s| s.to_lowercase().contains(needle))
}

fn body_contains(root: &Path, row: &WebChatRow, needle: &str) -> bool {
    row.body_fetched_at.is_some()
        && body(root, row).is_ok_and(|b| {
            b.messages
                .iter()
                .any(|m| m.text.to_lowercase().contains(needle))
        })
}

/// Newest first; searching bodies unpacks each stored body, so it runs only when asked.
pub fn list(conn: &Connection, root: &Path, q: &WebQuery) -> Result<WebPage, String> {
    let needle = q.search.trim().to_lowercase();
    let matching: Vec<WebChatRow> = chat_rows(conn)?
        .into_iter()
        .filter(|r| q.site.is_empty() || r.site == q.site)
        .filter(|r| q.account.is_empty() || r.account == q.account)
        .filter(|r| {
            needle.is_empty()
                || matches_fields(r, &needle)
                || (q.full_text && body_contains(root, r, &needle))
        })
        .collect();
    Ok(WebPage {
        total: matching.len(),
        items: matching
            .into_iter()
            .skip(q.offset)
            .take(WEB_PAGE_SIZE)
            .collect(),
        accounts: account_options(conn)?,
    })
}

/// Saves a summary and remembers which body it was written from.
pub fn save_summary(conn: &Connection, key: &str, text: &str, by: &str, at: i64) -> Result<(), String> {
    conn.execute(
        "UPDATE web_conversations SET summary=?2, summary_by=?3, summary_at=?4, summary_body_at=body_updated_at WHERE key=?1",
        params![key, text, by, at],
    )
    .map(|_| ())
    .map_err(db_err)
}
```

（`Result::is_ok_and` 自 Rust 1.70 可用。）

- [ ] **Step 4: 实现命令**

`src-tauri/src/sessions/commands.rs`：`async fn blocking<T: Send + 'static>(` 改为 `pub(crate) async fn blocking<T: Send + 'static>(`；`fn explorer(arg: impl AsRef<std::ffi::OsStr>) -> Result<(), String> {` 改为 `pub(crate) fn explorer(arg: impl AsRef<std::ffi::OsStr>) -> Result<(), String> {`。

`src-tauri/src/webchat/mod.rs`：加 `pub mod commands;`。

`src-tauri/src/webchat/commands.rs`（测试模块之前）：

```rust
//! Tauri commands for the 网页对话 tab and the 浏览器插件 settings block.
use super::host::{self, Browser, UserRegistry};
use super::protocol::WebMessage;
use super::store::{self, WebChatRow, WebPage, WebQuery};
use crate::runner::CancelFlag;
use crate::sessions::commands::{blocking, explorer};
use crate::sessions::model::Agent;
use crate::sessions::summary::{self, RunnerChoice, SummarySettings};
use crate::sessions::summary_job::{live_runner, Runner};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebchatStatus {
    pub extension_id: String,
    pub extension_dir: String,
    pub extension_found: bool,
    pub data_dir: String,
    pub export_dir: String,
    pub browsers: Vec<host::BrowserStatus>,
    pub last_hello_at: Option<i64>,
    pub last_sync_at: Option<i64>,
    pub counts: store::Counts,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebChatDetail {
    pub chat: WebChatRow,
    pub messages: Vec<WebMessage>,
    /// Characters a summary would send.
    pub chars: usize,
    pub runner: RunnerChoice,
}

fn exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|_| "E_PATH".to_string())
}

fn status_now() -> Result<WebchatStatus, String> {
    let root = super::root();
    let conn = store::open(&root)?;
    let (dir, found) = host::extension_dir();
    Ok(WebchatStatus {
        extension_id: super::extension_id().to_string(),
        extension_dir: dir.to_string_lossy().into_owned(),
        extension_found: found,
        data_dir: root.to_string_lossy().into_owned(),
        export_dir: super::export_dir().join("web").to_string_lossy().into_owned(),
        browsers: host::status(&UserRegistry, &root, &exe()?),
        last_hello_at: store::meta(&conn, "last_hello_at"),
        last_sync_at: store::meta(&conn, "last_sync_at"),
        counts: store::counts(&conn)?,
    })
}

#[tauri::command]
pub async fn webchat_status() -> Result<WebchatStatus, String> {
    blocking(status_now).await
}

/// Writes the manifest and the browser's HKCU key; only ever called from the user's click.
#[tauri::command]
pub async fn webchat_connect(browser: String) -> Result<WebchatStatus, String> {
    blocking(move || {
        let browser = Browser::parse(&browser)?;
        host::connect(&UserRegistry, browser, &super::root(), &exe()?, super::extension_id())?;
        status_now()
    })
    .await
}

#[tauri::command]
pub async fn webchat_disconnect(browser: String) -> Result<WebchatStatus, String> {
    blocking(move || {
        host::disconnect(&UserRegistry, Browser::parse(&browser)?, &super::root())?;
        status_now()
    })
    .await
}

/// `target`: `extension` (the unpacked extension folder) or `exports` (web chat exports).
#[tauri::command]
pub async fn webchat_open(target: String) -> Result<(), String> {
    blocking(move || match target.as_str() {
        "extension" => {
            let (dir, found) = host::extension_dir();
            if !found {
                return Err("E_NO_EXTENSION".into());
            }
            explorer(dir)
        }
        "exports" => {
            let dir = super::export_dir().join("web");
            std::fs::create_dir_all(&dir).map_err(|_| "E_STORAGE".to_string())?;
            explorer(dir)
        }
        _ => Err("E_REQUEST".into()),
    })
    .await
}

#[tauri::command]
pub async fn webchat_list(query: WebQuery) -> Result<WebPage, String> {
    blocking(move || {
        let root = super::root();
        store::list(&store::open(&root)?, &root, &query)
    })
    .await
}

/// A web chat has no agent of its own: 「同源」 means Claude; a fixed runner is honoured.
pub fn runner_for(settings: &SummarySettings) -> RunnerChoice {
    summary::choose(settings, Agent::Claude)
}

/// Same heading shape as local sessions ("\n### "), so long chats split into parts the same way.
pub fn body_markdown(title: &str, messages: &[WebMessage]) -> String {
    let mut out = format!("# {title}\n\n");
    for m in messages {
        let heading = match m.role.as_str() {
            "user" => "用户",
            "assistant" => "助手",
            _ => "工具",
        };
        out.push_str(&format!("### {heading}\n\n{}\n\n", m.text));
    }
    out
}

fn saved_settings() -> Result<SummarySettings, String> {
    Ok(summary::load_settings(&crate::sessions::annotations::connect()?))
}

#[tauri::command]
pub async fn webchat_read(key: String) -> Result<WebChatDetail, String> {
    blocking(move || {
        let root = super::root();
        let conn = store::open(&root)?;
        let chat = store::chat(&conn, &key)?;
        let messages = if chat.body_fetched_at.is_some() {
            store::body(&root, &chat)?.messages
        } else {
            Vec::new()
        };
        let chars = if messages.is_empty() {
            0
        } else {
            body_markdown(&chat.title, &messages).chars().count()
        };
        Ok(WebChatDetail { runner: runner_for(&saved_settings()?), chat, messages, chars })
    })
    .await
}

pub fn summarize_in(
    root: &Path,
    key: &str,
    settings: &SummarySettings,
    locale: &str,
    cancel: &CancelFlag,
    run: &Runner,
) -> Result<WebChatRow, String> {
    let conn = store::open(root)?;
    let chat = store::chat(&conn, key)?;
    if chat.body_fetched_at.is_none() {
        return Err("E_NO_BODY".into());
    }
    let body = store::body(root, &chat)?;
    let choice = runner_for(settings);
    let markdown = body_markdown(&chat.title, &body.messages);
    let text = summary::summarize_text(&markdown, &choice, locale, cancel, run.as_ref())?;
    store::save_summary(&conn, key, &text, &choice.label(), super::now_ms())?;
    store::chat(&conn, key)
}

static SUMMARY: Mutex<Option<CancelFlag>> = Mutex::new(None);

/// Clears the running-summary slot however the summary ends, including a panic.
struct Running;

impl Drop for Running {
    fn drop(&mut self) {
        if let Ok(mut slot) = SUMMARY.lock() {
            *slot = None;
        }
    }
}

#[tauri::command]
pub async fn webchat_summarize(
    key: String,
    settings: Option<SummarySettings>,
    locale: String,
) -> Result<WebChatRow, String> {
    blocking(move || {
        let flag = CancelFlag::default();
        {
            let mut slot = SUMMARY.lock().map_err(|_| "E_SUMMARY_BUSY".to_string())?;
            if slot.is_some() {
                return Err("E_SUMMARY_BUSY".into());
            }
            *slot = Some(flag.clone());
        }
        let _running = Running;
        let settings = match settings {
            Some(s) => s,
            None => saved_settings()?,
        };
        summarize_in(&super::root(), &key, &settings, &locale, &flag, &live_runner())
    })
    .await
}

#[tauri::command]
pub fn webchat_summary_cancel() {
    if let Ok(slot) = SUMMARY.lock() {
        if let Some(flag) = slot.as_ref() {
            flag.cancel();
        }
    }
}
```

注意：`"用户"`、`"助手"`、`"工具"` 已在 `src/en.generated.ts` 中，`check:i18n` 无需新增。

`src-tauri/src/lib.rs`：删除 Task 1 加的两行注释与 `#[allow(dead_code)]`，只留 `mod webchat;`；在 `generate_handler!` 列表里 `sessions::commands::migration_cancel,` 之后加：

```rust
            webchat::commands::webchat_status,
            webchat::commands::webchat_connect,
            webchat::commands::webchat_disconnect,
            webchat::commands::webchat_open,
            webchat::commands::webchat_list,
            webchat::commands::webchat_read,
            webchat::commands::webchat_summarize,
            webchat::commands::webchat_summary_cancel,
```

- [ ] **Step 5: 运行，确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`、`cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`、`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`、`npm run check:i18n`
Expected: 全部通过；clippy 不再有 `dead_code`（去掉 allow 之后所有 webchat 项都已被使用；若仍有报出的项，说明它只被测试使用——把它移进 `#[cfg(test)]` 或删除，不要恢复 allow）。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/webchat src-tauri/src/sessions/commands.rs src-tauri/src/lib.rs
git commit -m "feat(webchat): list, search, summaries and Tauri commands" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 10: 随 Stacker 附带插件文件夹

**Files:**
- Create: `src-tauri/tauri.bundle.conf.json`
- Modify: `scripts/release-windows.ps1`

**Interfaces:**
- Consumes: `npm run ext:build`（输出 `extension/dist`）；Task 8 `host::extension_dir()` 查找 `<exe 目录>\extension`。
- Produces: 安装版与免安装版的 `stacker.exe` 旁边都有 `extension\manifest.json`。

- [ ] **Step 1: 资源配置**

`src-tauri/tauri.bundle.conf.json`：

```json
{
  "bundle": {
    "resources": {
      "../extension/dist/**/*": "extension/"
    }
  }
}
```

（不写进 `tauri.conf.json`：`extension/dist` 不入库，普通 `cargo build`、`cargo test`、CI 会因资源不存在而失败。Tauri CLI 用 `--config` 合并这个文件。）

- [ ] **Step 2: 发布脚本**

`scripts/release-windows.ps1`：把

```powershell
Invoke-Checked "Windows release build" { & npm.cmd run tauri -- build }
```

替换为：

```powershell
Invoke-Checked "Browser extension build" { & npm.cmd run ext:build }
Invoke-Checked "Windows release build" { & npm.cmd run tauri -- build --config src-tauri/tauri.bundle.conf.json }
```

在 `if (-not (Test-Path $NsisSource)) { throw "NSIS installer not found: $NsisSource" }` 之后加：

```powershell
$BundledExtension = Join-Path $Root "src-tauri\target\release\extension"
if (-not (Test-Path (Join-Path $BundledExtension "manifest.json"))) { throw "Browser extension was not bundled: $BundledExtension" }
if (-not (Test-Path (Join-Path $BundledExtension "chunks"))) { throw "Browser extension subfolders were not bundled: $BundledExtension" }
```

在 `Copy-Item (Join-Path $Root "resources\PORTABLE_README.txt") (Join-Path $PortableStage "README.txt")` 之后加：

```powershell
Copy-Item (Join-Path $Root "extension\dist") (Join-Path $PortableStage "extension") -Recurse
```

- [ ] **Step 3: 验证打包**

Run（PowerShell，耗时数分钟）：

```powershell
npm run ext:build
npm run tauri -- build --config src-tauri/tauri.bundle.conf.json
Test-Path src-tauri\target\release\extension\manifest.json
Test-Path src-tauri\target\release\extension\chunks
```

Expected: 两个 `True`。若 `chunks` 为 `False`（该 Tauri 版本的 glob 目标不保留子目录），把配置改为 `"../extension/dist/": "extension/"` 重试，直到两者都为 `True`。安装包里的文件位置由 NSIS 按同一资源表放到安装目录下 `extension\`。

- [ ] **Step 4: 确认普通构建不受影响**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`（无 `--config`）
Expected: 成功，即使先删除 `extension/dist` 也成功。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/tauri.bundle.conf.json scripts/release-windows.ps1
git commit -m "build: ship the browser extension folder with Stacker" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---
### Task 11: 「设置 → 浏览器插件」

**Files:**
- Create: `src/features/sessions/BrowserExtension.tsx`, `src/features/sessions/BrowserExtension.test.tsx`
- Modify: `src/features/sessions/types.ts`, `src/features/sessions/api.ts`, `src/features/sessions/SettingsPanel.tsx`, `src/features/sessions/sessions.css`, `src/en.generated.ts`

**Interfaces:**
- Consumes: Task 9 命令 `webchat_status`、`webchat_connect`、`webchat_disconnect`、`webchat_open`，以及 `webchat_list`、`webchat_read`、`webchat_summarize`、`webchat_summary_cancel`（Task 12 使用）。
- Produces（`src/features/sessions/types.ts`）：`WebBrowser`、`WebHostState`、`WebBrowserStatus`、`WebCounts`、`WebchatStatus`、`WebQuery`、`EMPTY_WEB_QUERY`、`WEB_PAGE_SIZE`、`WebChat`、`WebAccountOption`、`WebPage`、`WebMessage`、`WebChatDetail`、`webSiteLabel(site: string): string`；（`api.ts`）`webchatStatus()`、`webchatConnect(browser)`、`webchatDisconnect(browser)`、`webchatOpen(target)`、`listWebChats(query)`、`readWebChat(key)`、`summarizeWebChat(key, settings, locale)`、`cancelWebSummary()`；组件 `BrowserExtension()`。

- [ ] **Step 1: 类型与接口**

`src/features/sessions/types.ts` 的 `ERRORS` 对象里（`E_NOT_LINK` 一行之后）加：

```ts
  E_REGISTRY: "无法写入注册表，请检查当前用户的注册表权限。",
  E_NO_EXTENSION: "没有找到插件文件夹。",
  E_NO_BODY: "Stacker 还没有这条对话的正文。",
  E_SUMMARY_BUSY: "已有网页对话摘要正在生成，请等待完成。",
```

文件末尾追加：

```ts
export type WebBrowser = "chrome" | "edge";
export type WebHostState = "off" | "connected" | "stale";
export type WebBrowserStatus = { browser: WebBrowser; state: WebHostState; registered: string };
export type WebCounts = { accounts: number; conversations: number; bodies: number; folders: number; excerpts: number };
/** Web chat times are milliseconds (browser clocks), unlike local sessions. */
export type WebchatStatus = {
  extensionId: string;
  extensionDir: string;
  extensionFound: boolean;
  dataDir: string;
  exportDir: string;
  browsers: WebBrowserStatus[];
  lastHelloAt: number | null;
  lastSyncAt: number | null;
  counts: WebCounts;
};
export type WebQuery = { site: string; account: string; search: string; fullText: boolean; offset: number };
export const EMPTY_WEB_QUERY: WebQuery = { site: "", account: "", search: "", fullText: false, offset: 0 };
export const WEB_PAGE_SIZE = 100;
export type WebChat = {
  key: string;
  site: string;
  account: string;
  accountName: string;
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  archived: boolean;
  removedAt: number | null;
  folder: string | null;
  tags: string[];
  favorite: boolean;
  note: string;
  bodyFetchedAt: number | null;
  bodyMessages: number;
  bodyStale: boolean;
  summary: string | null;
  summaryBy: string;
  summaryAt: number;
  summaryStale: boolean;
};
export type WebAccountOption = { key: string; site: string; name: string };
export type WebPage = { items: WebChat[]; total: number; accounts: WebAccountOption[] };
export type WebMessage = { role: string; text: string; at: number | null; attachments: string[] };
export type WebChatDetail = { chat: WebChat; messages: WebMessage[]; chars: number; runner: RunnerChoice };

const WEB_SITE_LABEL: Record<string, string> = { chatgpt: "ChatGPT", claude: "Claude", gemini: "Gemini", grok: "Grok", deepseek: "DeepSeek" };
/** Site ids come from the extension; unknown ones are shown as they are. */
export const webSiteLabel = (site: string) => WEB_SITE_LABEL[site] ?? site;
```

`src/features/sessions/api.ts`：在 `from "./types"` 的类型 import 列表末尾追加 `WebBrowser, WebchatStatus, WebChat, WebChatDetail, WebPage, WebQuery`（`SummarySettings` 已在列表中）；文件末尾追加：

```ts
export const webchatStatus = () => invoke<WebchatStatus>("webchat_status");
export const webchatConnect = (browser: WebBrowser) => invoke<WebchatStatus>("webchat_connect", { browser });
export const webchatDisconnect = (browser: WebBrowser) => invoke<WebchatStatus>("webchat_disconnect", { browser });
export const webchatOpen = (target: "extension" | "exports") => invoke<void>("webchat_open", { target });
export const listWebChats = (query: WebQuery) => invoke<WebPage>("webchat_list", { query });
export const readWebChat = (key: string) => invoke<WebChatDetail>("webchat_read", { key });
export const summarizeWebChat = (key: string, settings: SummarySettings | null, locale: string) => invoke<WebChat>("webchat_summarize", { key, settings, locale });
export const cancelWebSummary = () => invoke<void>("webchat_summary_cancel");
```

- [ ] **Step 2: 写失败的测试**

`src/features/sessions/BrowserExtension.test.tsx`：

```tsx
// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { BrowserExtension } from "./BrowserExtension";
import type { WebchatStatus, WebHostState } from "./types";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));

const status = (chrome: WebHostState): WebchatStatus => ({
  extensionId: "a".repeat(32), extensionDir: "C:\\Stacker\\extension", extensionFound: true,
  dataDir: "C:\\data", exportDir: "C:\\data\\exports\\web",
  browsers: [{ browser: "chrome", state: chrome, registered: "" }, { browser: "edge", state: "off", registered: "" }],
  lastHelloAt: null, lastSyncAt: 1_700_000_000_000,
  counts: { accounts: 1, conversations: 12, bodies: 3, folders: 2, excerpts: 4 },
});

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

const button = (text: string) => [...host.querySelectorAll("button")].find((b) => b.textContent === text);

describe("browser extension settings", () => {
  it("shows the folder and counts, and asks before writing the registry", async () => {
    vi.mocked(invoke).mockImplementation(async (command: string) => command === "webchat_connect" ? status("connected") : status("off"));
    await act(async () => { root.render(<BrowserExtension />); });
    expect(host.textContent).toContain("C:\\Stacker\\extension");
    expect(host.textContent).toContain("对话 12");
    expect(host.textContent).toContain("从未");
    await act(async () => { button("连接")!.click(); });
    expect(vi.mocked(invoke).mock.calls.map(([c]) => c)).not.toContain("webchat_connect");
    expect(host.textContent).toContain("HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts\\com.stacker.webchat");
    const confirm = [...host.querySelectorAll(".modal button")].find((b) => b.textContent === "连接") as HTMLElement;
    await act(async () => { confirm.click(); });
    expect(invoke).toHaveBeenCalledWith("webchat_connect", { browser: "chrome" });
    expect(host.querySelector(".modal")).toBeNull();
    expect(host.textContent).toContain("已连接");
  });

  it("disconnects without asking", async () => {
    vi.mocked(invoke).mockImplementation(async (command: string) => command === "webchat_disconnect" ? status("off") : status("connected"));
    await act(async () => { root.render(<BrowserExtension />); });
    await act(async () => { button("断开")!.click(); });
    expect(invoke).toHaveBeenCalledWith("webchat_disconnect", { browser: "chrome" });
  });

  it("explains a stale registration", async () => {
    vi.mocked(invoke).mockResolvedValue(status("stale"));
    await act(async () => { root.render(<BrowserExtension />); });
    expect(host.textContent).toContain("登记指向其他位置，请重新连接");
  });
});
```

- [ ] **Step 3: 运行，确认失败**

Run: `npx vitest run src/features/sessions/BrowserExtension.test.tsx`
Expected: FAIL（模块不存在）。

- [ ] **Step 4: 实现组件**

`src/features/sessions/BrowserExtension.tsx`：

```tsx
import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ConfirmModal, useBusyRead } from "../../ui";
import { webchatConnect, webchatDisconnect, webchatOpen, webchatStatus } from "./api";
import { errorMessage, type WebBrowser, type WebchatStatus, type WebHostState } from "./types";

const BROWSER_LABEL: Record<WebBrowser, string> = { chrome: "Chrome", edge: "Edge" };
const REGISTRY_KEY: Record<WebBrowser, string> = {
  chrome: "HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts\\com.stacker.webchat",
  edge: "HKCU\\Software\\Microsoft\\Edge\\NativeMessagingHosts\\com.stacker.webchat",
};
const STATE_LABEL: Record<WebHostState, string> = { off: "未连接", connected: "已连接", stale: "登记指向其他位置，请重新连接" };

/** Settings block: where the extension folder is, how to load it, and the Chrome / Edge registration. */
export function BrowserExtension() {
  const { tr: t, locale } = useI18n();
  const read = useBusyRead();
  const [status, setStatus] = useState<WebchatStatus | null>(null);
  const [asking, setAsking] = useState<WebBrowser | null>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState("");

  const load = useCallback(() => {
    read("正在读取浏览器插件状态", webchatStatus)
      .then((next) => { setStatus(next); setError(""); })
      .catch((e) => setError(errorMessage(e)));
  }, [read]);
  useEffect(() => { load(); }, [load]);

  async function change(browser: WebBrowser, connect: boolean) {
    setWorking(true); setError("");
    try {
      setStatus(await (connect ? webchatConnect(browser) : webchatDisconnect(browser)));
      setAsking(null);
    } catch (e) { setError(errorMessage(e)); }
    finally { setWorking(false); }
  }

  const time = (ms: number | null) => (ms ? new Date(ms).toLocaleString(locale) : t("从未"));
  if (!status) return error ? <div className="session-source"><p role="alert" className="session-error">{t(error)}</p></div> : null;
  const c = status.counts;
  return <div className="session-source browser-extension">
    <div className="session-source-head">
      <b>{t("浏览器插件")}</b>
      <small>{t("「Stacker 网页对话」插件管理 ChatGPT、Claude 等网站上的对话。连接后，插件的对话列表、已读取的正文、文件夹、标签、备注和摘录会同步到 Stacker，可在「网页对话」标签查看、搜索和生成摘要；插件的导出也会直接存到 Stacker 的导出目录。")}</small>
    </div>
    <div className="session-source-edit">
      <code title={status.extensionDir}>{status.extensionDir}</code>
      <button className="gh sm" disabled={!status.extensionFound} onClick={() => void webchatOpen("extension").catch((e) => setError(errorMessage(e)))}><i className="ti ti-folder-open" />{t("打开文件夹")}</button>
    </div>
    {!status.extensionFound && <small className="warn">{t("没有找到插件文件夹。开发时请先运行 npm run ext:build。")}</small>}
    <ol className="session-note extension-steps">
      <li>{t("在 Chrome 打开 chrome://extensions，或在 Edge 打开 edge://extensions，打开「开发者模式」。")}</li>
      <li>{t("点「加载已解压的扩展程序」，选择上面的插件文件夹。")}</li>
      <li>{t("在下方点对应浏览器的「连接」，然后在扩展管理页点插件的「重新加载」。")}</li>
    </ol>
    {status.browsers.map((b) => <div className="data-location" key={b.browser}>
      <div className="data-location-text">
        <b>{BROWSER_LABEL[b.browser]}</b>
        <small className={b.state === "stale" ? "warn" : ""}>{t(STATE_LABEL[b.state])}</small>
      </div>
      <div className="session-actions">
        {b.state !== "connected" && <button className="pr sm" disabled={working} onClick={() => setAsking(b.browser)}>{t("连接")}</button>}
        {b.state !== "off" && <button className="gh sm" disabled={working} onClick={() => void change(b.browser, false)}>{t("断开")}</button>}
      </div>
    </div>)}
    <small className="session-note">{t("最近连接")}：{time(status.lastHelloAt)} · {t("最近同步")}：{time(status.lastSyncAt)}</small>
    <small className="session-note">{t("账号")} {c.accounts} · {t("对话")} {c.conversations} · {t("已存正文")} {c.bodies} · {t("文件夹")} {c.folders} · {t("摘录")} {c.excerpts}</small>
    {error && <p role="alert" className="session-error">{t(error)}</p>}
    {asking && <ConfirmModal title={`${t("连接")} ${BROWSER_LABEL[asking]}`} icon="ti-plug-connected" busy={working}
      message={`${t("将在当前用户的注册表写入")} ${REGISTRY_KEY[asking]}${t("，并在 Stacker 数据目录保存一个登记文件。浏览器插件之后可以启动 Stacker 同步数据，不会打开窗口。点「断开」会删除这两处。")}`}
      confirmLabel={t("连接")} onConfirm={() => void change(asking, true)} onClose={() => setAsking(null)} />}
  </div>;
}
```

`src/features/sessions/SettingsPanel.tsx`：顶部加 `import { BrowserExtension } from "./BrowserExtension";`；在「精简导出目录」那个 `<div className="session-source">` 之前插入 `<BrowserExtension />`。

`src/features/sessions/sessions.css` 末尾追加：

```css
.a .extension-steps{margin:0;padding-left:18px}
.a .extension-steps li{margin:2px 0}
```

- [ ] **Step 5: 英文**

`src/en.generated.ts` 的 `GENERATED_EN` 末尾（`};` 之前）追加（某键若已存在则跳过，不要重复）：

```ts
  "无法写入注册表，请检查当前用户的注册表权限。": "Cannot write the registry. Check the current user's registry permissions.",
  "没有找到插件文件夹。": "Extension folder not found.",
  "Stacker 还没有这条对话的正文。": "Stacker doesn't have this conversation's body yet.",
  "已有网页对话摘要正在生成，请等待完成。": "A web chat summary is already being generated; wait for it to finish.",
  "正在读取浏览器插件状态": "Reading browser extension status",
  "浏览器插件": "Browser extension",
  "「Stacker 网页对话」插件管理 ChatGPT、Claude 等网站上的对话。连接后，插件的对话列表、已读取的正文、文件夹、标签、备注和摘录会同步到 Stacker，可在「网页对话」标签查看、搜索和生成摘要；插件的导出也会直接存到 Stacker 的导出目录。": "The “Stacker Web Chats” extension manages conversations on ChatGPT, Claude and other sites. Once connected, its conversation list, fetched bodies, folders, tags, notes and excerpts sync to Stacker, where the Web chats tab lets you view, search and summarize them; the extension's exports are saved straight into Stacker's export folder.",
  "打开文件夹": "Open folder",
  "没有找到插件文件夹。开发时请先运行 npm run ext:build。": "Extension folder not found. In development, run npm run ext:build first.",
  "在 Chrome 打开 chrome://extensions，或在 Edge 打开 edge://extensions，打开「开发者模式」。": "Open chrome://extensions in Chrome or edge://extensions in Edge and turn on Developer mode.",
  "点「加载已解压的扩展程序」，选择上面的插件文件夹。": "Click “Load unpacked” and choose the extension folder above.",
  "在下方点对应浏览器的「连接」，然后在扩展管理页点插件的「重新加载」。": "Click Connect for that browser below, then click the extension's Reload button on the extensions page.",
  "未连接": "Not connected",
  "已连接": "Connected",
  "登记指向其他位置，请重新连接": "Registered to another location; connect again",
  "连接": "Connect",
  "断开": "Disconnect",
  "最近连接": "Last connected",
  "最近同步": "Last synced",
  "从未": "Never",
  "对话": "Conversations",
  "已存正文": "Bodies",
  "文件夹": "Folders",
  "摘录": "Excerpts",
  "将在当前用户的注册表写入": "This writes the current user's registry key",
  "，并在 Stacker 数据目录保存一个登记文件。浏览器插件之后可以启动 Stacker 同步数据，不会打开窗口。点「断开」会删除这两处。": " and saves a host manifest in Stacker's data folder. The browser extension can then start Stacker to sync, without opening a window. Disconnect removes both.",
```

- [ ] **Step 6: 运行，确认通过**

Run: `npx vitest run src/features/sessions`、`npm run typecheck`、`npm run lint`、`npm run check:i18n`
Expected: 全部通过。

- [ ] **Step 7: 提交**

```bash
git add src/features/sessions/BrowserExtension.tsx src/features/sessions/BrowserExtension.test.tsx src/features/sessions/types.ts src/features/sessions/api.ts src/features/sessions/SettingsPanel.tsx src/features/sessions/sessions.css src/en.generated.ts
git commit -m "feat(sessions): browser extension connection settings" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 12: 「网页对话」标签

**Files:**
- Create: `src/features/sessions/WebChatPanel.tsx`, `src/features/sessions/WebChatDetail.tsx`, `src/features/sessions/WebChatPanel.test.tsx`
- Modify: `src/features/sessions/SessionCatalog.tsx`, `src/features/sessions/sessions.css`, `src/en.generated.ts`

**Interfaces:**
- Consumes: Task 11 的类型与 `listWebChats`、`readWebChat`、`summarizeWebChat`、`cancelWebSummary`；`runnerText(c: RunnerChoice, t)`（`./SummaryDialog` 已导出）；`useBusy`、`useBusyRead`、`Modal`、`ConfirmModal`（`../../ui`）；`Select`（`../../Select`）。
- Produces: `WebChatPanel({ refresh }: { refresh: number })`；`WebChatDetail({ chat, onClose }: { chat: WebChat; onClose: (changed: boolean) => void })`；`SessionCatalog` 新标签 `"web"`。

- [ ] **Step 1: 写失败的测试**

`src/features/sessions/WebChatPanel.test.tsx`：

```tsx
// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import type { WebChat } from "./types";
import { WebChatPanel } from "./WebChatPanel";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));

const chat: WebChat = {
  key: "chatgpt:c1", site: "chatgpt", account: "chatgpt:u1", accountName: "Work", id: "c1", title: "Trip plan",
  createdAt: 1, updatedAt: 1_700_000_000_000, archived: false, removedAt: null, folder: "Trips", tags: ["travel"],
  favorite: false, note: "", bodyFetchedAt: 5, bodyMessages: 1, bodyStale: false,
  summary: null, summaryBy: "", summaryAt: 0, summaryStale: false,
};

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers(); vi.clearAllMocks();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  vi.mocked(invoke).mockImplementation(async (command: string) => {
    if (command === "webchat_list") return { items: [chat], total: 1, accounts: [{ key: "chatgpt:u1", site: "chatgpt", name: "Work" }] };
    if (command === "webchat_read") return { chat, messages: [{ role: "user", text: "Where should we go?", at: null, attachments: [] }], chars: 30, runner: { agent: "claude", model: "sonnet", effort: "low" } };
    if (command === "webchat_summarize") return { ...chat, summary: "Go to Kyoto", summaryBy: "claude / sonnet / low", summaryAt: 1_700_000_000_000 };
    return null;
  });
});
afterEach(() => { act(() => root.unmount()); host.remove(); vi.useRealTimers(); });

async function mount() {
  await act(async () => { root.render(<WebChatPanel refresh={0} />); });
  await act(async () => { await vi.advanceTimersByTimeAsync(350); });
}
async function click(el: Element | null | undefined) {
  expect(el).toBeTruthy();
  await act(async () => { (el as HTMLElement).click(); });
}
const lastQuery = () => (vi.mocked(invoke).mock.calls.filter(([c]) => c === "webchat_list").pop()?.[1] as { query: { search: string; fullText: boolean } }).query;

describe("web chats tab", () => {
  it("lists synced chats with their site, account and folder", async () => {
    await mount();
    expect(host.textContent).toContain("Trip plan");
    expect(host.textContent).toContain("ChatGPT · Work · Trips");
  });

  it("searches after a pause and can include bodies", async () => {
    await mount();
    const input = host.querySelector("input[aria-label='搜索网页对话']") as HTMLInputElement;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "kyoto");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await click(host.querySelector(".session-check input"));
    await act(async () => { await vi.advanceTimersByTimeAsync(350); });
    expect(lastQuery()).toMatchObject({ search: "kyoto", fullText: true });
  });

  it("asks before sending a body to the runner, then shows the summary", async () => {
    await mount();
    await click(host.querySelector(".session-title"));
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(host.textContent).toContain("Where should we go?");
    await click([...host.querySelectorAll("button")].find((b) => b.textContent?.includes("生成摘要")));
    expect(vi.mocked(invoke).mock.calls.map(([c]) => c)).not.toContain("webchat_summarize");
    expect(host.textContent).toContain("Claude · sonnet · low");
    const confirm = [...host.querySelectorAll(".modal button")].filter((b) => b.textContent === "生成摘要").pop();
    await click(confirm);
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(invoke).toHaveBeenCalledWith("webchat_summarize", { key: "chatgpt:c1", settings: null, locale: expect.any(String) });
    expect(host.textContent).toContain("Go to Kyoto");
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run src/features/sessions/WebChatPanel.test.tsx`
Expected: FAIL（模块不存在）。

- [ ] **Step 3: 实现**

`src/features/sessions/WebChatPanel.tsx`：

```tsx
import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { Select } from "../../Select";
import { useBusyRead } from "../../ui";
import { listWebChats } from "./api";
import { WebChatDetail } from "./WebChatDetail";
import { EMPTY_WEB_QUERY, errorMessage, WEB_PAGE_SIZE, webSiteLabel, type WebChat, type WebPage, type WebQuery } from "./types";

// Survives page switches within one app session, like the sessions tab.
let lastWebQuery = EMPTY_WEB_QUERY;

/** 网页对话: conversations synced from the browser extension. */
export function WebChatPanel({ refresh }: { refresh: number }) {
  const { tr: t, locale } = useI18n();
  const read = useBusyRead();
  const [query, setQuery] = useState<WebQuery>(lastWebQuery);
  const [page, setPage] = useState<WebPage | null>(null);
  const [error, setError] = useState("");
  const [open, setOpen] = useState<WebChat | null>(null);
  const request = useRef(0);

  const load = useCallback(async (q: WebQuery) => {
    const generation = ++request.current;
    try {
      const next = await read("正在读取网页对话", () => listWebChats(q));
      if (generation === request.current) { setPage(next); setError(""); }
    } catch (e) {
      if (generation === request.current) setError(errorMessage(e));
    }
  }, [read]);

  useEffect(() => {
    lastWebQuery = query;
    const timer = window.setTimeout(() => void load(query), 300);
    return () => clearTimeout(timer);
  }, [query, load, refresh]);

  const filter = (patch: Partial<WebQuery>) => setQuery((old) => ({ ...old, ...patch, offset: 0 }));
  const accounts = page?.accounts ?? [];
  const sites = [...new Set(accounts.map((a) => a.site))];
  const siteAccounts = accounts.filter((a) => !query.site || a.site === query.site);

  return <>
    <div className="session-filters">
      <label className="session-search"><i className="ti ti-search" /><input value={query.search} aria-label={t("搜索网页对话")} placeholder={t("搜索标题、备注、标签或摘要")} onChange={(e) => filter({ search: e.target.value })} /></label>
      <label className="session-check"><input type="checkbox" checked={query.fullText} onChange={(e) => filter({ fullText: e.target.checked })} />{t("搜索正文")}</label>
      <Select value={query.site} onChange={(site) => filter({ site, account: "" })} options={[{ value: "", label: t("全部站点") }, ...sites.map((s) => ({ value: s, label: webSiteLabel(s) }))]} />
      <Select value={query.account} onChange={(account) => filter({ account })} options={[{ value: "", label: t("全部账号") }, ...siteAccounts.map((a) => ({ value: a.key, label: `${webSiteLabel(a.site)} · ${a.name}` }))]} />
    </div>
    {error && <div role="alert" className="session-error">{t(error)}</div>}
    {page && page.total === 0 && <div className="session-empty">
      <i className="ti ti-world" />
      <b>{t("还没有网页对话")}</b>
      <span>{t("在「设置 → 浏览器插件」连接插件后，插件里的对话会同步到这里。")}</span>
    </div>}
    {page && page.total > 0 && <div className="session-list">
      {page.items.map((c) => <div key={c.key} className="session-item"><div className="web-chat-row">
        <button className="session-title" onClick={() => setOpen(c)}>
          <b>{c.favorite && <i className="ti ti-star" />}{c.title || t("（无标题）")}</b>
          <span>{[webSiteLabel(c.site), c.accountName, c.folder, c.tags.join("、")].filter(Boolean).join(" · ")}</span>
        </button>
        <div className="session-tags">
          {c.removedAt !== null && <span className="session-tag">{t("网站上已删除")}</span>}
          {c.bodyFetchedAt === null ? <span className="session-tag">{t("未存正文")}</span> : c.bodyStale ? <span className="session-tag">{t("正文不是最新")}</span> : null}
          {c.summary && <span className="session-tag">{t("有摘要")}</span>}
        </div>
        <div className="session-time">{new Date(c.updatedAt).toLocaleDateString(locale)}</div>
      </div></div>)}
    </div>}
    {page && page.total > WEB_PAGE_SIZE && <div className="session-pagination">
      <button className="gh sm" disabled={!query.offset} onClick={() => setQuery((q) => ({ ...q, offset: Math.max(0, q.offset - WEB_PAGE_SIZE) }))}>{t("上一页")}</button>
      <span>{query.offset + 1}–{Math.min(page.total, query.offset + WEB_PAGE_SIZE)} / {page.total}</span>
      <button className="gh sm" disabled={query.offset + WEB_PAGE_SIZE >= page.total} onClick={() => setQuery((q) => ({ ...q, offset: q.offset + WEB_PAGE_SIZE }))}>{t("下一页")}</button>
    </div>}
    {open && <WebChatDetail chat={open} onClose={(changed) => { setOpen(null); if (changed) void load(query); }} />}
  </>;
}
```

`src/features/sessions/WebChatDetail.tsx`：

```tsx
import { useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ConfirmModal, Modal, useBusy, useBusyRead } from "../../ui";
import { cancelWebSummary, readWebChat, summarizeWebChat } from "./api";
import { runnerText } from "./SummaryDialog";
import { errorMessage, webSiteLabel, type WebChat, type WebChatDetail as Detail } from "./types";

const ROLE: Record<string, string> = { user: "用户", assistant: "助手", tool: "工具", system: "系统" };

/** One web chat: its stored body, local notes and summary. */
export function WebChatDetail({ chat, onClose }: { chat: WebChat; onClose: (changed: boolean) => void }) {
  const { tr: t, locale } = useI18n();
  const read = useBusyRead();
  const busy = useBusy();
  const [detail, setDetail] = useState<Detail | null>(null);
  const [asking, setAsking] = useState(false);
  const [changed, setChanged] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    read("正在读取网页对话正文", () => readWebChat(chat.key))
      .then(setDetail)
      .catch((e) => setError(errorMessage(e)));
  }, [chat.key, read]);

  async function summarize() {
    setAsking(false); setError("");
    try {
      const updated = await busy({
        title: "正在生成摘要",
        message: "摘要由本机智能体生成，长对话需要几分钟。",
        cancel: { label: "取消", onCancel: () => void cancelWebSummary() },
      }, () => summarizeWebChat(chat.key, null, locale));
      setDetail((d) => (d ? { ...d, chat: updated } : d));
      setChanged(true);
    } catch (e) { setError(errorMessage(e)); }
  }

  const c = detail?.chat ?? chat;
  const hasBody = !!detail && detail.messages.length > 0;
  return <Modal wide title={c.title || t("（无标题）")} onClose={() => onClose(changed)}>
    <div className="session-detail-meta">
      <span>{webSiteLabel(c.site)} · {c.accountName}</span>
      <span>{new Date(c.updatedAt).toLocaleString(locale)}</span>
      {c.folder && <span>{c.folder}</span>}
      {c.tags.length > 0 && <span>{c.tags.join("、")}</span>}
      {c.removedAt !== null && <span>{t("网站上已删除")}</span>}
    </div>
    {c.note && <p className="session-note">{c.note}</p>}
    <div className="session-summary">
      <div className="session-summary-head">
        <b>{t("摘要")}{c.summaryStale && <em>{t("已过期")}</em>}</b>
        {c.summary && c.summaryBy && <small>{c.summaryBy} · {new Date(c.summaryAt).toLocaleString(locale)}</small>}
        <button className="gh sm" disabled={!hasBody} onClick={() => setAsking(true)}><i className="ti ti-sparkles" />{t(c.summary ? "重新生成" : "生成摘要")}</button>
      </div>
      {c.summary ? <pre translate="no">{c.summary}</pre> : <p className="session-note">{t("还没有摘要。")}</p>}
    </div>
    {error && <p role="alert" className="session-error">{t(error)}</p>}
    {detail && !hasBody && <p className="session-note">{t("Stacker 还没有这条对话的正文：在插件里打开它并点「读取正文」，同步后即可在这里查看和生成摘要。")}</p>}
    {hasBody && c.bodyStale && <p className="session-note">{t("网站上的对话在读取正文后又有更新，这里显示的是上次读取的内容。")}</p>}
    <div className="session-transcript" translate="no">
      {detail?.messages.map((m, i) => <article key={i} className={m.role}><header><b>{t(ROLE[m.role] ?? m.role)}</b></header><pre>{m.text}</pre></article>)}
    </div>
    {asking && detail && <ConfirmModal title={t("生成摘要")} icon="ti-sparkles"
      message={`${t("将把这条对话的正文")}（${detail.chars} ${t("字")}）${t("发送给")} ${runnerText(detail.runner, t)} ${t("生成摘要，消耗该账号的额度。执行者、模型与推理强度沿用「设置 → 摘要」。")}`}
      confirmLabel={t("生成摘要")} onConfirm={() => void summarize()} onClose={() => setAsking(false)} />}
  </Modal>;
}
```

`src/features/sessions/SessionCatalog.tsx`：
- import 区加 `import { WebChatPanel } from "./WebChatPanel";`
- `type Tab = "sessions" | "projects" | "footprint" | "sources";` 改为 `type Tab = "sessions" | "projects" | "web" | "footprint" | "sources";`
- `TABS` 改为：

```tsx
const TABS: [Tab, string, string][] = [["sessions", "会话", "ti-messages"], ["projects", "项目", "ti-folders"], ["web", "网页对话", "ti-world"], ["footprint", "占用", "ti-chart-pie"], ["sources", "设置", "ti-settings"]];
```

- 在 `const queryRef = useRef(query);` 之后加 `const [webRefresh, setWebRefresh] = useState(0);`
- 「刷新」按钮的 `onClick={() => void load()}` 改为 `onClick={() => { if (tab === "web") setWebRefresh((n) => n + 1); else void load(); }}`
- 在 `{tab === "projects" && …}` 之后加 `{tab === "web" && <WebChatPanel refresh={webRefresh} />}`

`src/features/sessions/sessions.css` 末尾追加：

```css
.a .web-chat-row{display:grid;grid-template-columns:minmax(160px,1fr) auto 90px;align-items:center;gap:10px;min-height:56px;padding:8px 10px;box-sizing:border-box}
.a .web-chat-row:hover{background:var(--card)}
```

- [ ] **Step 4: 英文**

`src/en.generated.ts` 末尾追加（已存在的跳过）：

```ts
  "网页对话": "Web chats",
  "正在读取网页对话": "Reading web chats",
  "搜索网页对话": "Search web chats",
  "搜索标题、备注、标签或摘要": "Search titles, notes, tags or summaries",
  "搜索正文": "Search bodies",
  "全部站点": "All sites",
  "全部账号": "All accounts",
  "还没有网页对话": "No web chats yet",
  "在「设置 → 浏览器插件」连接插件后，插件里的对话会同步到这里。": "Connect the extension under Settings → Browser extension; its conversations sync here.",
  "（无标题）": "(Untitled)",
  "网站上已删除": "Deleted on the site",
  "未存正文": "No body saved",
  "正文不是最新": "Body out of date",
  "有摘要": "Summarized",
  "系统": "System",
  "正在读取网页对话正文": "Reading web chat",
  "正在生成摘要": "Generating summary",
  "摘要由本机智能体生成，长对话需要几分钟。": "A local agent writes the summary; long conversations take a few minutes.",
  "还没有摘要。": "No summary yet.",
  "Stacker 还没有这条对话的正文：在插件里打开它并点「读取正文」，同步后即可在这里查看和生成摘要。": "Stacker doesn't have this conversation's body yet: open it in the extension and click “Read body”; once synced you can view and summarize it here.",
  "网站上的对话在读取正文后又有更新，这里显示的是上次读取的内容。": "The conversation changed on the site after its body was read; this shows the last read.",
  "将把这条对话的正文": "This sends the conversation's body",
  "发送给": "to",
  "生成摘要，消耗该账号的额度。执行者、模型与推理强度沿用「设置 → 摘要」。": "to write a summary, using that account's quota. Runner, model and reasoning effort follow Settings → Summary.",
```

（`用户`、`助手`、`工具`、`摘要`、`已过期`、`生成摘要`、`重新生成`、`字`、`取消`、`上一页`、`下一页` 已有英文。）

- [ ] **Step 5: 运行，确认通过**

Run: `npx vitest run src/features/sessions`、`npm run typecheck`、`npm run lint`、`npm run check:i18n`
Expected: 全部通过（原有 `SessionCatalog.test.tsx` 仍通过）。

- [ ] **Step 6: 提交**

```bash
git add src/features/sessions/WebChatPanel.tsx src/features/sessions/WebChatDetail.tsx src/features/sessions/WebChatPanel.test.tsx src/features/sessions/SessionCatalog.tsx src/features/sessions/sessions.css src/en.generated.ts
git commit -m "feat(sessions): web chats tab with search and summaries" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---
### Task 13: 插件 IndexedDB v2：outbox 与每处修改入队

**Files:**
- Modify: `extension/src/lib/db.ts`（整文件替换）、`extension/src/lib/db.test.ts`（追加）、`extension/public/manifest.json`、`extension/src/manifest.test.ts`

**Interfaces:**
- Consumes: 无新依赖。
- Produces（`extension/src/lib/db.ts`，在 G1 基础上）：
  - 记录新增字段：`Account.localUpdatedAt`、`Conversation.listedAt`、`Conversation.localUpdatedAt`、`Folder.localUpdatedAt`、`Excerpt.localUpdatedAt`（毫秒；v1 旧记录读出时可能缺失，按 0 处理）。
  - `DB_VERSION = 2`；`OutboxKind = "account" | "conversation" | "body" | "folder" | "excerpt" | "removeFolder" | "removeExcerpt" | "all"`；`OutboxEntry { seq?: number; kind: OutboxKind; key: string; at: number }`
  - `onOutboxChange(listener: (() => void) | null): void`；`outboxCount(db): Promise<number>`；`readOutbox(db, limit: number): Promise<OutboxEntry[]>`（按 seq 升序）；`dropOutbox(db, seqs: number[]): Promise<void>`；`enqueueAll(db, now: number): Promise<void>`（已有等待中的 `all` 时不再添加）
  - 签名变化（新增的 `now` 参数都有默认值 `Date.now()`，旧调用不变）：`renameAccount(db, key, alias, now?)`、`updateLocal(db, keys, patch, now?)`、`addTag(db, keys, tag, now?)`、`renameFolder(db, id, name, now?)`、`deleteFolder(db, id, now?)`、`deleteExcerpt(db, id, now?)`。
  - 恢复：`Backup { accounts: Account[]; folders: Folder[]; conversations: Omit<Conversation, "bodyFetchedAt" | "bodyUpdatedAt">[]; excerpts: Excerpt[] }`；`RestoreCounts { accounts; folders; conversations; excerpts: number }`；`applyBackup(db, backup: Backup): Promise<RestoreCounts>`（不入队）。

- [ ] **Step 1: manifest 权限**

`extension/public/manifest.json`：`"permissions": ["storage", "downloads"]` 改为 `"permissions": ["storage", "downloads", "nativeMessaging"]`。

`extension/src/manifest.test.ts`：`expect(manifest.permissions).toEqual(["storage", "downloads"]);` 改为 `expect(manifest.permissions).toEqual(["storage", "downloads", "nativeMessaging"]);`。（G2 并行修改同一测试的 `host_permissions` 行；合并时两处都保留。）

- [ ] **Step 2: 写失败的测试**

`extension/src/lib/db.test.ts`：import 改为

```ts
import "fake-indexeddb/auto";
import { openDB } from "idb";
import { beforeEach, describe, expect, it } from "vitest";
import {
  accountDisplayName, addExcerpt, addTag, applyBackup, bodyIsFresh, createFolder, deleteExcerpt, deleteFolder, dropOutbox,
  enqueueAll, getBody, getConversation, listAccounts, listConversations, listExcerpts, listFolders, markRemoved, mergeListing,
  onOutboxChange, openDb, outboxCount, putBody, readOutbox, renameAccount, renameFolder, updateLocal, upsertAccount,
  type Account, type Db,
} from "./db";
```

文件末尾追加：

```ts
const clearOutbox = async (d: Db) => dropOutbox(d, (await readOutbox(d, 1000)).map((e) => e.seq!));

describe("outbox", () => {
  it("upgrades a version 1 database and queues everything for the first sync", async () => {
    const name = `v1-${n++}`;
    const old = await openDB(name, 1, {
      upgrade(d) {
        d.createObjectStore("accounts", { keyPath: "key" });
        d.createObjectStore("conversations", { keyPath: "key" }).createIndex("account", "account");
        d.createObjectStore("bodies", { keyPath: "key" });
        d.createObjectStore("folders", { keyPath: "id" });
        d.createObjectStore("excerpts", { keyPath: "id" }).createIndex("conversation", ["site", "conversationId"]);
      },
    });
    await old.put("conversations", {
      key: "chatgpt:a", site: "chatgpt", account: "chatgpt:u", id: "a", title: "A", createdAt: 1, updatedAt: 2,
      archived: false, folderId: null, tags: [], favorite: false, note: "kept", bodyFetchedAt: null, bodyUpdatedAt: null, removedAt: null,
    });
    old.close();
    const upgraded = await openDb(name);
    expect((await readOutbox(upgraded, 10)).map((e) => e.kind)).toEqual(["all"]);
    expect((await getConversation(upgraded, "chatgpt:a"))?.note).toBe("kept");
  });

  it("queues every local change, and only real changes", async () => {
    await clearOutbox(db);
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 1);
    await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 2);
    await renameAccount(db, account.key, "Work", 3);
    await mergeListing(db, account, [item("a"), item("b")], true, 4);
    await mergeListing(db, account, [item("a"), item("b")], true, 5);
    await updateLocal(db, ["chatgpt:a"], { note: "x" }, 6);
    await addTag(db, ["chatgpt:a"], "t", 7);
    await addTag(db, ["chatgpt:a"], "t", 8);
    await putBody(db, "chatgpt:a", { id: "a", title: "A", updatedAt: 2, messages: [] }, 9);
    const folder = await createFolder(db, "F", 10);
    await renameFolder(db, folder.id, "G", 11);
    await deleteFolder(db, folder.id, 12);
    const excerpt = await addExcerpt(db, { site: "chatgpt", conversationId: "a", url: "u", pageTitle: "p", text: "t", note: "" }, 13);
    await deleteExcerpt(db, excerpt.id, 14);
    await markRemoved(db, "chatgpt:b", 15);
    expect((await readOutbox(db, 100)).map((e) => `${e.kind}:${e.key}`)).toEqual([
      "account:chatgpt:u", "account:chatgpt:u",
      "conversation:chatgpt:a", "conversation:chatgpt:b",
      "conversation:chatgpt:a", "conversation:chatgpt:a",
      "body:chatgpt:a",
      `folder:${folder.id}`, `folder:${folder.id}`, `removeFolder:${folder.id}`,
      `excerpt:${excerpt.id}`, `removeExcerpt:${excerpt.id}`,
      "conversation:chatgpt:b",
    ]);
    const a = (await getConversation(db, "chatgpt:a"))!;
    expect([a.listedAt, a.localUpdatedAt]).toEqual([5, 7]);
    expect((await getConversation(db, "chatgpt:b"))?.listedAt).toBe(15);
    expect((await listAccounts(db))[0].localUpdatedAt).toBe(3);
  });

  it("queues the conversations a deleted folder releases", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 1);
    await mergeListing(db, account, [item("a")], true, 1);
    const folder = await createFolder(db, "F", 2);
    await updateLocal(db, ["chatgpt:a"], { folderId: folder.id }, 3);
    await clearOutbox(db);
    await deleteFolder(db, folder.id, 4);
    expect((await readOutbox(db, 10)).map((e) => e.kind)).toEqual(["removeFolder", "conversation"]);
    expect((await getConversation(db, "chatgpt:a"))?.localUpdatedAt).toBe(4);
  });

  it("keeps at most one 'send everything' entry waiting", async () => {
    await enqueueAll(db, 1);
    await enqueueAll(db, 2);
    expect((await readOutbox(db, 10)).map((e) => e.kind)).toEqual(["all"]);
  });

  it("tells the registered listener after a queued change", async () => {
    let calls = 0;
    onOutboxChange(() => { calls++; });
    await createFolder(db, "F", 1);
    onOutboxChange(null);
    await createFolder(db, "G", 2);
    expect(calls).toBe(1);
  });
});

describe("applyBackup", () => {
  const wire = (id: string) => ({
    key: `chatgpt:${id}`, site: "chatgpt" as const, account: "chatgpt:u", id, title: id, createdAt: 1, updatedAt: 2,
    archived: false, removedAt: null, listedAt: 1, folderId: null, tags: [] as string[], favorite: false, note: "", localUpdatedAt: 0,
  });

  it("adds what is missing and takes only newer local fields, without queueing", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 1);
    await mergeListing(db, account, [item("a")], true, 1);
    await updateLocal(db, ["chatgpt:a"], { note: "local newer" }, 50);
    await clearOutbox(db);
    const counts = await applyBackup(db, {
      accounts: [{ key: "chatgpt:u", site: "chatgpt", remoteId: "u", name: "Ada", alias: "Work", lastSeen: 1, localUpdatedAt: 40 }],
      folders: [{ id: "f1", name: "Trips", createdAt: 1, localUpdatedAt: 1 }],
      conversations: [
        { ...wire("a"), note: "from stacker", localUpdatedAt: 10 },
        { ...wire("b"), folderId: "f1", tags: ["travel"], localUpdatedAt: 20 },
        { ...wire("x"), key: "elsewhere:x", site: "elsewhere" as never },
      ],
      excerpts: [{ id: "e1", site: "chatgpt", conversationId: "b", url: "u", pageTitle: "p", text: "tip", note: "", createdAt: 1, localUpdatedAt: 1 }],
    });
    expect(counts).toEqual({ accounts: 1, folders: 1, conversations: 1, excerpts: 1 });
    expect((await getConversation(db, "chatgpt:a"))?.note).toBe("local newer");
    expect(await getConversation(db, "chatgpt:b")).toMatchObject({ folderId: "f1", tags: ["travel"], bodyFetchedAt: null, bodyUpdatedAt: null });
    expect(await getConversation(db, "elsewhere:x")).toBeUndefined();
    expect((await listAccounts(db))[0].alias).toBe("Work");
    expect((await listFolders(db)).map((f) => f.name)).toEqual(["Trips"]);
    expect(await outboxCount(db)).toBe(0);
  });
});
```

（`item`、`n`、`db` 是该文件顶部已有的辅助变量。）

- [ ] **Step 3: 运行，确认失败**

Run: `npx vitest run extension/src/lib/db.test.ts extension/src/manifest.test.ts`
Expected: FAIL（`readOutbox` 等未导出；manifest 测试在 Step 1 之后通过）。

- [ ] **Step 4: 实现**

`extension/src/lib/db.ts` 整文件替换为：

```ts
import { openDB, type DBSchema, type IDBPDatabase } from "idb";
import type { RemoteAccount, RemoteBody, RemoteConversation, SiteId } from "../shared/types";
import { SITES } from "../sites/registry";

export interface Account { key: string; site: SiteId; remoteId: string; name: string; alias: string; lastSeen: number; localUpdatedAt: number }
export interface LocalFields { folderId: string | null; tags: string[]; favorite: boolean; note: string }
export interface Conversation extends LocalFields {
  key: string; site: SiteId; account: string; id: string; title: string;
  createdAt: number; updatedAt: number; archived: boolean;
  bodyFetchedAt: number | null; bodyUpdatedAt: number | null; removedAt: number | null;
  /** When the site's own fields (title, times, archived, removed) were last written; Stacker keeps the newest. */
  listedAt: number;
  /** When a local field (folder, tags, favorite, note) last changed; the newer copy wins on sync and restore. */
  localUpdatedAt: number;
}
export interface StoredBody extends RemoteBody { key: string }
export interface Folder { id: string; name: string; createdAt: number; localUpdatedAt: number }
export interface Excerpt { id: string; site: SiteId; conversationId: string | null; url: string; pageTitle: string; text: string; note: string; createdAt: number; localUpdatedAt: number }

/** A change waiting for Stacker. Only the record's key is queued; the current record is read when sending. */
export type OutboxKind = "account" | "conversation" | "body" | "folder" | "excerpt" | "removeFolder" | "removeExcerpt" | "all";
export interface OutboxEntry { seq?: number; kind: OutboxKind; key: string; at: number }

interface Schema extends DBSchema {
  accounts: { key: string; value: Account };
  conversations: { key: string; value: Conversation; indexes: { account: string } };
  bodies: { key: string; value: StoredBody };
  folders: { key: string; value: Folder };
  excerpts: { key: string; value: Excerpt; indexes: { conversation: [SiteId, string] } };
  outbox: { key: number; value: OutboxEntry };
}
export type Db = IDBPDatabase<Schema>;
export const DB_VERSION = 2;

export function openDb(name = "stacker-web"): Promise<Db> {
  return openDB<Schema>(name, DB_VERSION, {
    upgrade(db, oldVersion, _newVersion, tx) {
      if (oldVersion < 1) {
        db.createObjectStore("accounts", { keyPath: "key" });
        db.createObjectStore("conversations", { keyPath: "key" }).createIndex("account", "account");
        db.createObjectStore("bodies", { keyPath: "key" });
        db.createObjectStore("folders", { keyPath: "id" });
        db.createObjectStore("excerpts", { keyPath: "id" }).createIndex("conversation", ["site", "conversationId"]);
      }
      if (oldVersion < 2) {
        db.createObjectStore("outbox", { keyPath: "seq", autoIncrement: true });
        // Everything saved before sync existed goes to Stacker on the first connection.
        void tx.objectStore("outbox").add({ kind: "all", key: "", at: Date.now() });
      }
    },
  });
}

export const conversationKey = (site: SiteId, id: string) => `${site}:${id}`;
export const accountKey = (site: SiteId, remoteId: string) => `${site}:${remoteId}`;
const newId = () => crypto.randomUUID();
/** Records saved before version 2 have no sync times; they count as oldest. */
const stamp = (value: number | undefined) => value ?? 0;

let outboxListener: (() => void) | null = null;
/** Pages and the background say how to start a sync after a local change. */
export function onOutboxChange(listener: (() => void) | null): void { outboxListener = listener; }
const changed = () => outboxListener?.();

type OutboxWriter = { add(value: OutboxEntry): Promise<unknown> };
async function enqueue(store: OutboxWriter, kind: OutboxKind, keys: string[], at: number): Promise<void> {
  for (const key of keys) await store.add({ kind, key, at });
}

/** A stored alias auto-generated by an older version, back when it stood in for the site name. */
const AUTO_ALIAS = /^(ChatGPT|Claude)( \d+)?$/;

/** Records saved before `name` existed: an alias that was really just the site label becomes no alias at all. */
function migrateAccount(a: Account, site: SiteId): Account {
  const synced = { ...a, localUpdatedAt: stamp(a.localUpdatedAt) };
  if (typeof a.name === "string" && a.name) return synced;
  return { ...synced, name: SITES[site].label, alias: AUTO_ALIAS.test(a.alias) ? "" : a.alias };
}

export async function upsertAccount(db: Db, site: SiteId, remote: RemoteAccount, now: number): Promise<Account> {
  const key = accountKey(site, remote.remoteId);
  const tx = db.transaction(["accounts", "outbox"], "readwrite");
  const existing = await tx.objectStore("accounts").get(key);
  const name = remote.label || SITES[site].label;
  const account: Account = existing
    ? { ...migrateAccount(existing, site), name, lastSeen: now }
    : { key, site, remoteId: remote.remoteId, name, alias: "", lastSeen: now, localUpdatedAt: 0 };
  await tx.objectStore("accounts").put(account);
  const queued = !existing || existing.name !== name;
  if (queued) await enqueue(tx.objectStore("outbox"), "account", [key], now);
  await tx.done;
  if (queued) changed();
  return account;
}

export async function listAccounts(db: Db): Promise<Account[]> {
  return (await db.getAll("accounts")).map((a) => migrateAccount(a, a.site));
}

/** The name shown for an account: the user's own alias when set, else the site's latest name for it. */
export function accountDisplayName(a: Account): string {
  return a.alias || a.name;
}

export async function renameAccount(db: Db, key: string, alias: string, now = Date.now()): Promise<void> {
  const tx = db.transaction(["accounts", "outbox"], "readwrite");
  const a = await tx.objectStore("accounts").get(key);
  if (a) {
    await tx.objectStore("accounts").put({ ...migrateAccount(a, a.site), alias: alias.trim(), localUpdatedAt: now });
    await enqueue(tx.objectStore("outbox"), "account", [key], now);
  }
  await tx.done;
  if (a) changed();
}

/** Refreshes titles and times, keeps local fields; a complete listing marks the rest removed. */
export async function mergeListing(db: Db, account: Account, items: RemoteConversation[], complete: boolean, now: number) {
  const tx = db.transaction(["conversations", "outbox"], "readwrite");
  const store = tx.objectStore("conversations");
  const counts = { added: 0, updated: 0, removed: 0 };
  const seen = new Set<string>();
  const queued: string[] = [];
  for (const item of items) {
    const key = conversationKey(account.site, item.id);
    seen.add(key);
    const old = await store.get(key);
    if (!old) { counts.added++; queued.push(key); }
    else if (old.updatedAt !== item.updatedAt || old.title !== item.title || old.archived !== item.archived || old.removedAt !== null) {
      counts.updated++;
      queued.push(key);
    }
    await store.put({
      folderId: null, tags: [], favorite: false, note: "", bodyFetchedAt: null, bodyUpdatedAt: null, localUpdatedAt: 0,
      ...old,
      key, site: account.site, account: account.key, id: item.id, title: item.title,
      createdAt: item.createdAt, updatedAt: item.updatedAt, archived: item.archived, removedAt: null, listedAt: now,
    });
  }
  if (complete) {
    for (const c of await store.index("account").getAll(account.key)) {
      if (!seen.has(c.key) && c.removedAt === null) {
        counts.removed++;
        queued.push(c.key);
        await store.put({ ...c, removedAt: now, listedAt: now });
      }
    }
  }
  await enqueue(tx.objectStore("outbox"), "conversation", queued, now);
  await tx.done;
  if (queued.length) changed();
  return counts;
}

export const listConversations = (db: Db) => db.getAll("conversations");
export const getConversation = (db: Db, key: string) => db.get("conversations", key);

export async function updateLocal(db: Db, keys: string[], patch: Partial<LocalFields>, now = Date.now()): Promise<void> {
  const tx = db.transaction(["conversations", "outbox"], "readwrite");
  const store = tx.objectStore("conversations");
  const queued: string[] = [];
  for (const key of keys) {
    const c = await store.get(key);
    if (c) { await store.put({ ...c, ...patch, localUpdatedAt: now }); queued.push(key); }
  }
  await enqueue(tx.objectStore("outbox"), "conversation", queued, now);
  await tx.done;
  if (queued.length) changed();
}

export async function addTag(db: Db, keys: string[], tag: string, now = Date.now()): Promise<void> {
  const clean = tag.trim();
  if (!clean) return;
  const tx = db.transaction(["conversations", "outbox"], "readwrite");
  const store = tx.objectStore("conversations");
  const queued: string[] = [];
  for (const key of keys) {
    const c = await store.get(key);
    if (c && !c.tags.includes(clean)) { await store.put({ ...c, tags: [...c.tags, clean], localUpdatedAt: now }); queued.push(key); }
  }
  await enqueue(tx.objectStore("outbox"), "conversation", queued, now);
  await tx.done;
  if (queued.length) changed();
}

export async function putBody(db: Db, key: string, body: RemoteBody, now: number): Promise<void> {
  const tx = db.transaction(["bodies", "conversations", "outbox"], "readwrite");
  await tx.objectStore("bodies").put({ ...body, key });
  const c = await tx.objectStore("conversations").get(key);
  if (c) await tx.objectStore("conversations").put({ ...c, bodyFetchedAt: now, bodyUpdatedAt: body.updatedAt });
  await enqueue(tx.objectStore("outbox"), "body", [key], now);
  await tx.done;
  changed();
}

export const getBody = (db: Db, key: string) => db.get("bodies", key);

export function bodyIsFresh(c: Conversation): boolean {
  return c.bodyUpdatedAt !== null && c.bodyUpdatedAt >= c.updatedAt;
}

export async function markRemoved(db: Db, key: string, now: number): Promise<void> {
  const tx = db.transaction(["conversations", "outbox"], "readwrite");
  const c = await tx.objectStore("conversations").get(key);
  if (c) {
    await tx.objectStore("conversations").put({ ...c, removedAt: now, listedAt: now });
    await enqueue(tx.objectStore("outbox"), "conversation", [key], now);
  }
  await tx.done;
  if (c) changed();
}

export const listFolders = (db: Db) => db.getAll("folders");

export async function createFolder(db: Db, name: string, now: number): Promise<Folder> {
  const folder = { id: newId(), name: name.trim(), createdAt: now, localUpdatedAt: now };
  const tx = db.transaction(["folders", "outbox"], "readwrite");
  await tx.objectStore("folders").put(folder);
  await enqueue(tx.objectStore("outbox"), "folder", [folder.id], now);
  await tx.done;
  changed();
  return folder;
}

export async function renameFolder(db: Db, id: string, name: string, now = Date.now()): Promise<void> {
  if (!name.trim()) return;
  const tx = db.transaction(["folders", "outbox"], "readwrite");
  const f = await tx.objectStore("folders").get(id);
  if (f) {
    await tx.objectStore("folders").put({ ...f, name: name.trim(), localUpdatedAt: now });
    await enqueue(tx.objectStore("outbox"), "folder", [id], now);
  }
  await tx.done;
  if (f) changed();
}

export async function deleteFolder(db: Db, id: string, now = Date.now()): Promise<void> {
  const tx = db.transaction(["folders", "conversations", "outbox"], "readwrite");
  await tx.objectStore("folders").delete(id);
  const released: string[] = [];
  for (const c of await tx.objectStore("conversations").getAll()) {
    if (c.folderId === id) {
      await tx.objectStore("conversations").put({ ...c, folderId: null, localUpdatedAt: now });
      released.push(c.key);
    }
  }
  const outbox = tx.objectStore("outbox");
  await enqueue(outbox, "removeFolder", [id], now);
  await enqueue(outbox, "conversation", released, now);
  await tx.done;
  changed();
}

export async function addExcerpt(db: Db, e: Omit<Excerpt, "id" | "createdAt" | "localUpdatedAt">, now: number): Promise<Excerpt> {
  const excerpt = { ...e, id: newId(), createdAt: now, localUpdatedAt: now };
  const tx = db.transaction(["excerpts", "outbox"], "readwrite");
  await tx.objectStore("excerpts").put(excerpt);
  await enqueue(tx.objectStore("outbox"), "excerpt", [excerpt.id], now);
  await tx.done;
  changed();
  return excerpt;
}

export async function listExcerpts(db: Db, site?: SiteId, conversationId?: string): Promise<Excerpt[]> {
  let all: Excerpt[];
  if (site && conversationId) {
    all = await db.getAllFromIndex("excerpts", "conversation", [site, conversationId]);
  } else if (site) {
    const allExcerpts = await db.getAll("excerpts");
    all = allExcerpts.filter((e) => e.site === site);
  } else {
    all = await db.getAll("excerpts");
  }
  return all.sort((a, b) => b.createdAt - a.createdAt);
}

export async function deleteExcerpt(db: Db, id: string, now = Date.now()): Promise<void> {
  const tx = db.transaction(["excerpts", "outbox"], "readwrite");
  await tx.objectStore("excerpts").delete(id);
  await enqueue(tx.objectStore("outbox"), "removeExcerpt", [id], now);
  await tx.done;
  changed();
}

export const outboxCount = (db: Db) => db.count("outbox");
export const readOutbox = (db: Db, limit: number) => db.getAll("outbox", undefined, limit);

export async function dropOutbox(db: Db, seqs: number[]): Promise<void> {
  if (!seqs.length) return;
  const tx = db.transaction("outbox", "readwrite");
  for (const seq of seqs) await tx.store.delete(seq);
  await tx.done;
}

/** Sends everything again, e.g. when Stacker's copy is empty. At most one such entry waits at a time. */
export async function enqueueAll(db: Db, now: number): Promise<void> {
  const tx = db.transaction("outbox", "readwrite");
  let waiting = false;
  for (let cursor = await tx.store.openCursor(); cursor; cursor = await cursor.continue()) {
    if (cursor.value.kind === "all") { waiting = true; break; }
  }
  if (!waiting) await tx.store.add({ kind: "all", key: "", at: now });
  await tx.done;
  if (!waiting) changed();
}

export interface Backup {
  accounts: Account[];
  folders: Folder[];
  conversations: Omit<Conversation, "bodyFetchedAt" | "bodyUpdatedAt">[];
  excerpts: Excerpt[];
}
export interface RestoreCounts { accounts: number; folders: number; conversations: number; excerpts: number }

const knownSite = (site: string): site is SiteId => Object.prototype.hasOwnProperty.call(SITES, site);

/**
 * Brings organizing data back from Stacker: missing records are added, and a record's local
 * fields change only where Stacker's copy is newer. Nothing is queued, since it came from Stacker.
 */
export async function applyBackup(db: Db, backup: Backup): Promise<RestoreCounts> {
  const tx = db.transaction(["accounts", "folders", "conversations", "excerpts"], "readwrite");
  const counts: RestoreCounts = { accounts: 0, folders: 0, conversations: 0, excerpts: 0 };
  const accounts = tx.objectStore("accounts");
  for (const a of backup.accounts.filter((x) => knownSite(x.site))) {
    const local = await accounts.get(a.key);
    if (!local) { await accounts.put(a); counts.accounts++; }
    else if (a.localUpdatedAt > stamp(local.localUpdatedAt)) {
      await accounts.put({ ...local, alias: a.alias, localUpdatedAt: a.localUpdatedAt });
      counts.accounts++;
    }
  }
  const folders = tx.objectStore("folders");
  for (const f of backup.folders) {
    const local = await folders.get(f.id);
    if (!local) { await folders.put(f); counts.folders++; }
    else if (f.localUpdatedAt > stamp(local.localUpdatedAt)) {
      await folders.put({ ...local, name: f.name, localUpdatedAt: f.localUpdatedAt });
      counts.folders++;
    }
  }
  const conversations = tx.objectStore("conversations");
  for (const c of backup.conversations.filter((x) => knownSite(x.site))) {
    const local = await conversations.get(c.key);
    if (!local) { await conversations.put({ ...c, bodyFetchedAt: null, bodyUpdatedAt: null }); counts.conversations++; }
    else if (c.localUpdatedAt > stamp(local.localUpdatedAt)) {
      await conversations.put({ ...local, folderId: c.folderId, tags: c.tags, favorite: c.favorite, note: c.note, localUpdatedAt: c.localUpdatedAt });
      counts.conversations++;
    }
  }
  const excerpts = tx.objectStore("excerpts");
  for (const e of backup.excerpts.filter((x) => knownSite(x.site))) {
    const local = await excerpts.get(e.id);
    if (!local) { await excerpts.put(e); counts.excerpts++; }
    else if (e.localUpdatedAt > stamp(local.localUpdatedAt)) {
      await excerpts.put({ ...local, text: e.text, note: e.note, localUpdatedAt: e.localUpdatedAt });
      counts.excerpts++;
    }
  }
  await tx.done;
  return counts;
}
```

`addExcerpt` 的参数类型去掉了 `localUpdatedAt`，`background.ts` 的调用不用改。

- [ ] **Step 5: 运行，确认通过**

Run: `npx vitest run extension/src`、`npm run typecheck`、`npm run lint`
Expected: 全部通过（原有 db、deleteJob、refresh、Popup 测试不变）。

- [ ] **Step 6: 提交**

```bash
git add extension/public/manifest.json extension/src/manifest.test.ts extension/src/lib/db.ts extension/src/lib/db.test.ts
git commit -m "feat(extension): IndexedDB v2 outbox that queues every local change" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 14: 分批与 flush

**Files:**
- Create: `extension/src/lib/sync.ts`, `extension/src/lib/sync.test.ts`

**Interfaces:**
- Consumes: Task 13 的 `readOutbox`、`dropOutbox`、`outboxCount`、`listAccounts`、`listConversations`、`listFolders`、`listExcerpts`、`getConversation`、`getBody` 与记录类型。
- Produces（`extension/src/lib/sync.ts`）：
  - `type Call = (type: string, payload: unknown) => Promise<unknown>`
  - `MAX_BATCH = 200`；`MAX_BYTES = 900_000`；`MAX_MESSAGE_CHARS = 250_000`；`FLUSH_LIMIT = 2000`
  - `byteSize(value: unknown): number`；`batches<T>(items: T[], max?: number, bytes?: number): T[][]`
  - `BodyChunk { key; site; account; id; title; updatedAt; fetchedAt; chunk: number; chunks: number; messages: Message[] }`；`bodyChunks(c: Conversation, body: StoredBody, fetchedAt: number): BodyChunk[]`
  - `Outgoing`（`{ type: "syncAccounts" | "syncFolders" | "syncConversations" | "syncExcerpts" | "removeRecords"; payload: { items: unknown[] }; seqs: number[] }` 或 `{ type: "syncBody"; key: string; seqs: number[] }`）；`buildRequests(db, entries: OutboxEntry[]): Promise<Outgoing[]>`
  - `flush(db, call: Call, limit?: number): Promise<number>`（返回发出的消息数；某条失败即抛出，剩余留在队列）

- [ ] **Step 1: 写失败的测试**

`extension/src/lib/sync.test.ts`：

```ts
import "fake-indexeddb/auto";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  addExcerpt, createFolder, deleteExcerpt, dropOutbox, enqueueAll, mergeListing, openDb, outboxCount, putBody, readOutbox,
  updateLocal, upsertAccount, type Conversation, type Db,
} from "./db";
import { batches, bodyChunks, byteSize, flush, MAX_BYTES, MAX_MESSAGE_CHARS, type Call } from "./sync";

let db: Db;
let n = 0;
beforeEach(async () => {
  db = await openDb(`sync-${n++}`);
  await dropOutbox(db, (await readOutbox(db, 1000)).map((e) => e.seq!));
});
const item = (id: string) => ({ id, title: id, createdAt: 1, updatedAt: 2, archived: false });
const conv = { key: "chatgpt:a", site: "chatgpt", account: "chatgpt:u", id: "a" } as Conversation;

describe("batches", () => {
  it("splits by count and by size", () => {
    expect(batches([1, 2, 3, 4, 5], 2).map((b) => b.length)).toEqual([2, 2, 1]);
    const big = "x".repeat(400);
    expect(batches([big, big, big], 10, 1000).map((b) => b.length)).toEqual([2, 1]);
    expect(batches([])).toEqual([]);
  });
});

describe("bodyChunks", () => {
  it("splits a large body by message and keeps every chunk under the limit", () => {
    const messages = Array.from({ length: 12 }, (_, i) => ({ role: "user" as const, text: `${i}`.padEnd(200_000, "字"), at: null, attachments: [] }));
    const chunks = bodyChunks(conv, { key: "chatgpt:a", id: "a", title: "A", updatedAt: 2, messages }, 9);
    expect(chunks.length).toBeGreaterThan(1);
    chunks.forEach((c, i) => {
      expect([c.chunk, c.chunks, c.fetchedAt]).toEqual([i, chunks.length, 9]);
      expect(byteSize(c)).toBeLessThan(MAX_BYTES);
    });
    expect(chunks.flatMap((c) => c.messages)).toEqual(messages);
  });

  it("clips a message that could never fit and sends an empty body as one chunk", () => {
    const huge = { role: "assistant" as const, text: "y".repeat(MAX_MESSAGE_CHARS + 10), at: null, attachments: [] };
    const [only] = bodyChunks(conv, { key: "chatgpt:a", id: "a", title: "A", updatedAt: 2, messages: [huge] }, 1);
    expect(only.messages[0].text.endsWith("…[truncated]")).toBe(true);
    expect(bodyChunks(conv, { key: "chatgpt:a", id: "a", title: "A", updatedAt: 2, messages: [] }, 1))
      .toMatchObject([{ chunk: 0, chunks: 1, messages: [] }]);
  });
});

describe("flush", () => {
  it("sends each changed record once, in order, and empties the outbox", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 1);
    await mergeListing(db, account, [item("a")], true, 2);
    await updateLocal(db, ["chatgpt:a"], { note: "one" }, 3);
    await updateLocal(db, ["chatgpt:a"], { note: "two" }, 4);
    await putBody(db, "chatgpt:a", { id: "a", title: "A", updatedAt: 2, messages: [{ role: "user", text: "hi", at: null, attachments: [] }] }, 5);
    await createFolder(db, "F", 6);
    const excerpt = await addExcerpt(db, { site: "chatgpt", conversationId: "a", url: "u", pageTitle: "p", text: "t", note: "" }, 7);
    await deleteExcerpt(db, excerpt.id, 8);
    const call = vi.fn<Call>(async () => ({}));
    expect(await flush(db, call)).toBe(5);
    expect(call.mock.calls.map(([type]) => type)).toEqual(["syncAccounts", "syncFolders", "syncConversations", "syncBody", "removeRecords"]);
    const conversations = call.mock.calls[2][1] as { items: { key: string; note: string; listedAt: number; localUpdatedAt: number }[] };
    expect(conversations.items).toEqual([expect.objectContaining({ key: "chatgpt:a", note: "two", listedAt: 2, localUpdatedAt: 4 })]);
    expect(call.mock.calls[3][1]).toMatchObject({ key: "chatgpt:a", fetchedAt: 5, chunk: 0, chunks: 1 });
    expect(call.mock.calls[4][1]).toEqual({ items: [{ kind: "excerpt", key: excerpt.id, at: 8 }] });
    expect(await outboxCount(db)).toBe(0);
  });

  it("keeps what failed in the outbox and sends it next time", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 1);
    await mergeListing(db, account, [item("a")], true, 2);
    const failing = vi.fn<Call>(async (type) => {
      if (type === "syncConversations") throw new Error("E_NOT_CONNECTED");
      return {};
    });
    await expect(flush(db, failing)).rejects.toThrow("E_NOT_CONNECTED");
    expect((await readOutbox(db, 10)).map((e) => e.kind)).toEqual(["conversation"]);
    const ok = vi.fn<Call>(async () => ({}));
    await flush(db, ok);
    expect(ok.mock.calls.map(([type]) => type)).toEqual(["syncConversations"]);
    expect(await outboxCount(db)).toBe(0);
  });

  it("sends every record for an 'all' entry, 200 conversations per message", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "Ada" }, 1);
    await mergeListing(db, account, Array.from({ length: 250 }, (_, i) => item(`c${i}`)), true, 2);
    await dropOutbox(db, (await readOutbox(db, 1000)).map((e) => e.seq!));
    await enqueueAll(db, 3);
    const call = vi.fn<Call>(async () => ({}));
    await flush(db, call);
    const sizes = call.mock.calls.filter(([type]) => type === "syncConversations").map(([, p]) => (p as { items: unknown[] }).items.length);
    expect(sizes).toEqual([200, 50]);
    expect(call.mock.calls[0][0]).toBe("syncAccounts");
    expect(await outboxCount(db)).toBe(0);
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/lib/sync.test.ts`
Expected: FAIL（`./sync` 不存在）。

- [ ] **Step 3: 实现**

`extension/src/lib/sync.ts`：

```ts
import type { Message } from "../shared/types";
import {
  dropOutbox, getBody, getConversation, listAccounts, listConversations, listExcerpts, listFolders, readOutbox,
  type Account, type Conversation, type Db, type Excerpt, type Folder, type OutboxEntry, type OutboxKind, type StoredBody,
} from "./db";

export type Call = (type: string, payload: unknown) => Promise<unknown>;

/** Records per message, as Stacker accepts them. */
export const MAX_BATCH = 200;
/** Stays under Chrome's 1 MB native-message limit with room for the envelope. */
export const MAX_BYTES = 900_000;
/** A single message longer than this is clipped so its chunk still fits. */
export const MAX_MESSAGE_CHARS = 250_000;
export const FLUSH_LIMIT = 2000;

const encoder = new TextEncoder();
export const byteSize = (value: unknown) => encoder.encode(JSON.stringify(value)).length;
const stamp = (value: number | undefined) => value ?? 0;

export function batches<T>(items: T[], max = MAX_BATCH, bytes = MAX_BYTES): T[][] {
  const out: T[][] = [];
  let current: T[] = [];
  let size = 0;
  for (const item of items) {
    const itemSize = byteSize(item) + 1;
    if (current.length && (current.length >= max || size + itemSize > bytes)) {
      out.push(current);
      current = [];
      size = 0;
    }
    current.push(item);
    size += itemSize;
  }
  if (current.length) out.push(current);
  return out;
}

const wireAccount = (a: Account) => ({
  key: a.key, site: a.site, remoteId: a.remoteId, name: a.name, alias: a.alias, lastSeen: a.lastSeen, localUpdatedAt: stamp(a.localUpdatedAt),
});
const wireConversation = (c: Conversation) => ({
  key: c.key, site: c.site, account: c.account, id: c.id, title: c.title, createdAt: c.createdAt, updatedAt: c.updatedAt,
  archived: c.archived, removedAt: c.removedAt, listedAt: stamp(c.listedAt), folderId: c.folderId, tags: c.tags,
  favorite: c.favorite, note: c.note, localUpdatedAt: stamp(c.localUpdatedAt),
});
const wireFolder = (f: Folder) => ({ id: f.id, name: f.name, createdAt: f.createdAt, localUpdatedAt: stamp(f.localUpdatedAt) });
const wireExcerpt = (e: Excerpt) => ({
  id: e.id, site: e.site, conversationId: e.conversationId, url: e.url, pageTitle: e.pageTitle, text: e.text, note: e.note,
  createdAt: e.createdAt, localUpdatedAt: stamp(e.localUpdatedAt),
});

export interface BodyChunk {
  key: string; site: string; account: string; id: string; title: string; updatedAt: number; fetchedAt: number;
  chunk: number; chunks: number; messages: Message[];
}

const clip = (m: Message): Message =>
  m.text.length > MAX_MESSAGE_CHARS ? { ...m, text: `${m.text.slice(0, MAX_MESSAGE_CHARS)}\n…[truncated]` } : m;

/** One `syncBody` message per chunk; a long body is split by message index so each chunk fits. */
export function bodyChunks(c: Conversation, body: StoredBody, fetchedAt: number): BodyChunk[] {
  const head = { key: c.key, site: c.site, account: c.account, id: c.id, title: body.title, updatedAt: body.updatedAt, fetchedAt };
  const groups = batches(body.messages.map(clip), Number.MAX_SAFE_INTEGER, MAX_BYTES - byteSize(head) - 100);
  const parts = groups.length ? groups : [[]];
  return parts.map((messages, chunk) => ({ ...head, chunk, chunks: parts.length, messages }));
}

type BatchType = "syncAccounts" | "syncFolders" | "syncConversations" | "syncExcerpts" | "removeRecords";
export type Outgoing =
  | { type: BatchType; payload: { items: unknown[] }; seqs: number[] }
  | { type: "syncBody"; key: string; seqs: number[] };

const isDefined = <T,>(value: T | undefined): value is T => value !== undefined;

/** Turns queued keys into messages, reading each record as it is now. Order matters: folders and conversations before bodies. */
export async function buildRequests(db: Db, entries: OutboxEntry[]): Promise<Outgoing[]> {
  const all = entries.some((e) => e.kind === "all");
  const keysOf = (kind: OutboxKind) => [...new Set(entries.filter((e) => e.kind === kind).map((e) => e.key))];
  const seqsOf = (kinds: OutboxKind[], keys: string[]) =>
    entries.filter((e) => kinds.includes(e.kind) && keys.includes(e.key)).map((e) => e.seq!);
  const out: Outgoing[] = [];
  const addBatches = <T,>(type: BatchType, kinds: OutboxKind[], records: T[], keyOf: (r: T) => string, wire: (r: T) => unknown) => {
    for (const group of batches(records.map((r) => ({ key: keyOf(r), wire: wire(r) })))) {
      out.push({ type, payload: { items: group.map((g) => g.wire) }, seqs: seqsOf(kinds, group.map((g) => g.key)) });
    }
  };

  const accounts = all ? await listAccounts(db) : (await Promise.all(keysOf("account").map((k) => db.get("accounts", k)))).filter(isDefined);
  addBatches("syncAccounts", ["account"], accounts, (a) => a.key, wireAccount);
  const folders = all ? await listFolders(db) : (await Promise.all(keysOf("folder").map((k) => db.get("folders", k)))).filter(isDefined);
  addBatches("syncFolders", ["folder"], folders, (f) => f.id, wireFolder);
  const conversations = all ? await listConversations(db) : (await Promise.all(keysOf("conversation").map((k) => getConversation(db, k)))).filter(isDefined);
  addBatches("syncConversations", ["conversation"], conversations, (c) => c.key, wireConversation);
  const bodyKeys = all ? conversations.filter((c) => c.bodyFetchedAt !== null).map((c) => c.key) : keysOf("body");
  for (const key of bodyKeys) out.push({ type: "syncBody", key, seqs: seqsOf(["body"], [key]) });
  const excerpts = all ? await listExcerpts(db) : (await Promise.all(keysOf("excerpt").map((k) => db.get("excerpts", k)))).filter(isDefined);
  addBatches("syncExcerpts", ["excerpt"], excerpts, (e) => e.id, wireExcerpt);
  const removals = entries
    .filter((e) => e.kind === "removeFolder" || e.kind === "removeExcerpt")
    .map((e) => ({ kind: e.kind === "removeFolder" ? "folder" : "excerpt", key: e.key, at: e.at, seq: e.seq! }));
  for (const group of batches(removals)) {
    out.push({ type: "removeRecords", payload: { items: group.map(({ kind, key, at }) => ({ kind, key, at })) }, seqs: group.map((r) => r.seq) });
  }

  const allSeqs = entries.filter((e) => e.kind === "all").map((e) => e.seq!);
  if (allSeqs.length && out.length) out[out.length - 1].seqs.push(...allSeqs);
  return out;
}

async function loadBodyChunks(db: Db, key: string): Promise<BodyChunk[]> {
  const [c, body] = await Promise.all([getConversation(db, key), getBody(db, key)]);
  return c && body ? bodyChunks(c, body, c.bodyFetchedAt ?? Date.now()) : [];
}

/**
 * Sends queued changes to Stacker in order. Each message's queue entries are dropped only after
 * Stacker accepted it; the first failure stops the flush and leaves the rest queued.
 */
export async function flush(db: Db, call: Call, limit = FLUSH_LIMIT): Promise<number> {
  let sent = 0;
  for (;;) {
    const entries = await readOutbox(db, limit);
    if (!entries.length) return sent;
    const outgoing = await buildRequests(db, entries);
    const covered = new Set(outgoing.flatMap((o) => o.seqs));
    // Entries whose record no longer exists (e.g. an excerpt added then deleted) have nothing to send.
    await dropOutbox(db, entries.map((e) => e.seq!).filter((s) => !covered.has(s)));
    for (const o of outgoing) {
      if (o.type === "syncBody") {
        for (const chunk of await loadBodyChunks(db, o.key)) { await call("syncBody", chunk); sent++; }
      } else {
        await call(o.type, o.payload);
        sent++;
      }
      await dropOutbox(db, o.seqs);
    }
    if (entries.length < limit) return sent;
  }
}
```

注意：`<T,>` 泛型箭头函数写法在 `.ts` 文件中合法；若 lint 报风格问题改成 `function` 声明。

- [ ] **Step 4: 运行，确认通过**

Run: `npx vitest run extension/src/lib/sync.test.ts`、`npm run typecheck`、`npm run lint`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add extension/src/lib/sync.ts extension/src/lib/sync.test.ts
git commit -m "feat(extension): batch queued changes into bridge messages under 1 MB" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 15: 本地消息端口管理

**Files:**
- Create: `extension/src/lib/bridge.ts`, `extension/src/lib/bridge.test.ts`

**Interfaces:**
- Produces（`extension/src/lib/bridge.ts`）：
  - `HOST_NAME = "com.stacker.webchat"`；`REQUEST_TIMEOUT_MS = 30_000`；`IDLE_CLOSE_MS = 120_000`；`RETRY_AFTER_MS = 30_000`
  - `class BridgeError extends Error { code: string }`（`E_NOT_CONNECTED`、`E_TIMEOUT` 或 Stacker 返回的错误码）
  - `interface NativePort { postMessage(message: unknown): void; disconnect(): void; onMessage: { addListener(fn: (message: unknown) => void): void }; onDisconnect: { addListener(fn: (reason: string) => void): void } }`
  - `interface HelloResult { app: string; version: string; protocol: number; counts: { accounts: number; conversations: number; bodies: number; folders: number; excerpts: number } }`
  - `interface BridgeOptions { timeoutMs?; idleMs?; retryMs?; now?: () => number; version?: string; onConnected?: (hello: HelloResult) => void }`
  - `interface Bridge { call(type: string, payload?: unknown): Promise<unknown>; connected(): boolean; lastError(): string; reset(): void }`
  - `createBridge(connect: () => NativePort, options?: BridgeOptions): Bridge`；`connectChrome(): NativePort`（包装 `chrome.runtime.connectNative(HOST_NAME)`）

- [ ] **Step 1: 写失败的测试**

`extension/src/lib/bridge.test.ts`：

```ts
import { afterEach, describe, expect, it, vi } from "vitest";
import { createBridge, type HelloResult, type NativePort } from "./bridge";

type Sent = { id: string; type: string; payload: unknown };
type Reply = (req: Sent) => unknown;

const HELLO: HelloResult = { app: "stacker", version: "0.3.3", protocol: 1, counts: { accounts: 0, conversations: 0, bodies: 0, folders: 0, excerpts: 0 } };

class FakePort implements NativePort {
  reply: Reply;
  sent: Sent[] = [];
  disconnected = false;
  private messageListeners: ((m: unknown) => void)[] = [];
  private disconnectListeners: ((reason: string) => void)[] = [];
  constructor(reply: Reply) { this.reply = reply; }
  onMessage = { addListener: (fn: (m: unknown) => void) => { this.messageListeners.push(fn); } };
  onDisconnect = { addListener: (fn: (reason: string) => void) => { this.disconnectListeners.push(fn); } };
  postMessage(message: unknown) {
    const req = message as Sent;
    this.sent.push(req);
    const answer = this.reply(req);
    if (answer !== undefined) queueMicrotask(() => this.messageListeners.forEach((l) => l(answer)));
  }
  disconnect() { this.disconnected = true; }
  drop(reason: string) { this.disconnectListeners.forEach((l) => l(reason)); }
}

function fakeHost(reply: Reply) {
  const ports: FakePort[] = [];
  const connect = vi.fn(() => { const p = new FakePort(reply); ports.push(p); return p; });
  return { connect, ports };
}
const answering: Reply = (req) => ({ id: req.id, ok: true, result: req.type === "hello" ? HELLO : { echo: req.type } });

afterEach(() => { vi.useRealTimers(); });

describe("bridge", () => {
  it("says hello once, then sends each request with its own id", async () => {
    const { connect, ports } = fakeHost(answering);
    const onConnected = vi.fn();
    const bridge = createBridge(connect, { onConnected });
    expect(await bridge.call("status")).toEqual({ echo: "status" });
    expect(await bridge.call("pullBackup", { section: "accounts", offset: 0 })).toEqual({ echo: "pullBackup" });
    expect(connect).toHaveBeenCalledTimes(1);
    expect(ports[0].sent.map((m) => m.type)).toEqual(["hello", "status", "pullBackup"]);
    expect(new Set(ports[0].sent.map((m) => m.id)).size).toBe(3);
    expect(bridge.connected()).toBe(true);
    expect(onConnected).toHaveBeenCalledWith(HELLO);
  });

  it("passes Stacker's error code through", async () => {
    const { connect } = fakeHost((req) => req.type === "hello" ? answering(req) : { id: req.id, ok: false, error: "E_PATH" });
    const bridge = createBridge(connect);
    await expect(bridge.call("saveExport", { path: "../x.md" })).rejects.toMatchObject({ code: "E_PATH" });
    expect(bridge.connected()).toBe(true);
  });

  it("treats a missing host as standalone and waits before trying again", async () => {
    let now = 0;
    const { connect, ports } = fakeHost(() => undefined);
    const bridge = createBridge(connect, { now: () => now, retryMs: 1000 });
    const first = bridge.call("status");
    ports[0].drop("Specified native messaging host not found.");
    await expect(first).rejects.toMatchObject({ code: "E_NOT_CONNECTED" });
    expect(bridge.connected()).toBe(false);
    expect(bridge.lastError()).toContain("not found");
    await expect(bridge.call("status")).rejects.toMatchObject({ code: "E_NOT_CONNECTED" });
    expect(connect).toHaveBeenCalledTimes(1);
    bridge.reset();
    const retried = bridge.call("status");
    ports[1].drop("Specified native messaging host not found.");
    await expect(retried).rejects.toMatchObject({ code: "E_NOT_CONNECTED" });
    now = 1000;
    const later = bridge.call("status");
    ports[2].drop("gone");
    await expect(later).rejects.toMatchObject({ code: "E_NOT_CONNECTED" });
    expect(connect).toHaveBeenCalledTimes(3);
  });

  it("times out when Stacker does not answer", async () => {
    vi.useFakeTimers();
    const { connect } = fakeHost((req) => req.type === "hello" ? answering(req) : undefined);
    const bridge = createBridge(connect, { timeoutMs: 100 });
    const pending = bridge.call("status");
    const check = expect(pending).rejects.toMatchObject({ code: "E_TIMEOUT" });
    await vi.advanceTimersByTimeAsync(100);
    await check;
  });

  it("closes an idle connection and reconnects on the next call", async () => {
    vi.useFakeTimers();
    const { connect, ports } = fakeHost(answering);
    const bridge = createBridge(connect, { idleMs: 1000 });
    await bridge.call("status");
    await vi.advanceTimersByTimeAsync(1000);
    expect(ports[0].disconnected).toBe(true);
    expect(bridge.connected()).toBe(false);
    await bridge.call("status");
    expect(connect).toHaveBeenCalledTimes(2);
    expect(ports[1].sent[0].type).toBe("hello");
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/lib/bridge.test.ts`
Expected: FAIL（模块不存在）。

- [ ] **Step 3: 实现**

`extension/src/lib/bridge.ts`：

```ts
/** Native messaging with Stacker: connect on demand, one request per id, standalone when unavailable. */
export const HOST_NAME = "com.stacker.webchat";
export const REQUEST_TIMEOUT_MS = 30_000;
export const IDLE_CLOSE_MS = 120_000;
export const RETRY_AFTER_MS = 30_000;

export class BridgeError extends Error {
  code: string;
  constructor(code: string) {
    super(code);
    this.code = code;
  }
}

export interface NativePort {
  postMessage(message: unknown): void;
  disconnect(): void;
  onMessage: { addListener(fn: (message: unknown) => void): void };
  onDisconnect: { addListener(fn: (reason: string) => void): void };
}

export interface HelloResult {
  app: string;
  version: string;
  protocol: number;
  counts: { accounts: number; conversations: number; bodies: number; folders: number; excerpts: number };
}

export interface BridgeOptions {
  timeoutMs?: number;
  idleMs?: number;
  retryMs?: number;
  now?: () => number;
  version?: string;
  onConnected?: (hello: HelloResult) => void;
}

export interface Bridge {
  call(type: string, payload?: unknown): Promise<unknown>;
  connected(): boolean;
  lastError(): string;
  /** Forget a recent failure so the next call tries at once. */
  reset(): void;
}

interface Pending { resolve: (value: unknown) => void; reject: (error: unknown) => void; timer: ReturnType<typeof setTimeout> }

export function createBridge(connect: () => NativePort, options: BridgeOptions = {}): Bridge {
  const timeoutMs = options.timeoutMs ?? REQUEST_TIMEOUT_MS;
  const idleMs = options.idleMs ?? IDLE_CLOSE_MS;
  const retryMs = options.retryMs ?? RETRY_AFTER_MS;
  const now = options.now ?? Date.now;
  let port: NativePort | null = null;
  let ready = false;
  let opening: Promise<void> | null = null;
  let failedAt = Number.NEGATIVE_INFINITY;
  let error = "";
  let seq = 0;
  let idleTimer: ReturnType<typeof setTimeout> | undefined;
  const pending = new Map<string, Pending>();

  function lost(reason: string) {
    port = null;
    ready = false;
    error = reason || "E_NOT_CONNECTED";
    failedAt = now();
    clearTimeout(idleTimer);
    for (const p of pending.values()) { clearTimeout(p.timer); p.reject(new BridgeError("E_NOT_CONNECTED")); }
    pending.clear();
  }

  function send(p: NativePort, type: string, payload: unknown): Promise<unknown> {
    const id = String(++seq);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { pending.delete(id); reject(new BridgeError("E_TIMEOUT")); }, timeoutMs);
      pending.set(id, { resolve, reject, timer });
      try {
        p.postMessage({ id, type, payload });
      } catch {
        clearTimeout(timer);
        pending.delete(id);
        reject(new BridgeError("E_NOT_CONNECTED"));
      }
    });
  }

  function onMessage(message: unknown) {
    const m = message as { id?: unknown; ok?: unknown; result?: unknown; error?: unknown } | null;
    if (!m || typeof m.id !== "string") return;
    const entry = pending.get(m.id);
    if (!entry) return;
    pending.delete(m.id);
    clearTimeout(entry.timer);
    if (m.ok === true) entry.resolve(m.result);
    else entry.reject(new BridgeError(typeof m.error === "string" ? m.error : "E_BRIDGE"));
  }

  async function open(): Promise<void> {
    let p: NativePort;
    try {
      p = connect();
    } catch (e) {
      lost(e instanceof Error ? e.message : String(e));
      throw new BridgeError("E_NOT_CONNECTED");
    }
    port = p;
    p.onMessage.addListener(onMessage);
    p.onDisconnect.addListener((reason) => { if (port === p) lost(reason); });
    try {
      const hello = (await send(p, "hello", { version: options.version ?? "" })) as HelloResult;
      if (port !== p) throw new BridgeError("E_NOT_CONNECTED");
      ready = true;
      error = "";
      options.onConnected?.(hello);
    } catch (e) {
      if (port === p) {
        p.disconnect();
        lost(e instanceof BridgeError ? e.code : String(e));
      }
      throw new BridgeError("E_NOT_CONNECTED");
    }
  }

  async function ensure(): Promise<NativePort> {
    if (port && ready) return port;
    if (!opening) {
      if (now() - failedAt < retryMs) throw new BridgeError("E_NOT_CONNECTED");
      opening = open().finally(() => { opening = null; });
    }
    await opening;
    if (!port) throw new BridgeError("E_NOT_CONNECTED");
    return port;
  }

  function touch() {
    clearTimeout(idleTimer);
    idleTimer = setTimeout(() => {
      if (pending.size) { touch(); return; }
      const p = port;
      port = null;
      ready = false;
      p?.disconnect();
    }, idleMs);
  }

  return {
    async call(type, payload = {}) {
      const p = await ensure();
      touch();
      return send(p, type, payload);
    },
    connected: () => port !== null && ready,
    lastError: () => error,
    reset: () => { failedAt = Number.NEGATIVE_INFINITY; },
  };
}

/** Chrome's port, adapted: the disconnect reason comes from `chrome.runtime.lastError`. */
export function connectChrome(): NativePort {
  const port = chrome.runtime.connectNative(HOST_NAME);
  return {
    postMessage: (message) => port.postMessage(message),
    disconnect: () => port.disconnect(),
    onMessage: { addListener: (fn) => port.onMessage.addListener((message: unknown) => fn(message)) },
    onDisconnect: { addListener: (fn) => port.onDisconnect.addListener(() => fn(chrome.runtime.lastError?.message ?? "")) },
  };
}
```

说明：「连接失败后 30 秒内不重试」的判断在 `ensure()`；测试里 `reset()` 之后立即重试成功发起第二次连接，`now` 前进到 `retryMs` 后发起第三次。

- [ ] **Step 4: 运行，确认通过**

Run: `npx vitest run extension/src/lib/bridge.test.ts`、`npm run typecheck`、`npm run lint`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add extension/src/lib/bridge.ts extension/src/lib/bridge.test.ts
git commit -m "feat(extension): native messaging port with timeouts, idle close and standalone fallback" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 16: 后台接线与页面消息

**Files:**
- Create: `extension/src/lib/bridgeMessages.ts`, `extension/src/lib/bridgeMessages.test.ts`
- Modify: `extension/src/background.ts`, `extension/src/ui/manage/main.tsx`, `extension/src/ui/popup/main.tsx`

**Interfaces:**
- Consumes: Task 13 `onOutboxChange`、`outboxCount`、`enqueueAll`、`openDb`；Task 14 `flush`；Task 15 `createBridge`、`connectChrome`、`BridgeError`、`Bridge`。
- Produces（`extension/src/lib/bridgeMessages.ts`）：
  - `BridgeStatus { connected: boolean; pending: number; lastSyncAt: number | null; error: string }`
  - `StackerCall = "saveExport" | "pullBackup"`
  - `BridgeMessage = { type: "bridge-status"; connect: boolean; force: boolean } | { type: "bridge-flush" } | { type: "bridge-call"; call: StackerCall; payload: unknown }`
  - `BridgeReply = { ok: true; value: unknown } | { ok: false; error: string }`
  - `isBridgeMessage(m: unknown): m is BridgeMessage`（页面只能调用 `saveExport`、`pullBackup`）
  - `createBridgeHandler(deps: { bridge: Bridge; openDb: () => Promise<Db>; flushSoon: (delayMs: number) => void; lastSyncAt: () => number | null }): (m: BridgeMessage) => Promise<BridgeReply>`
  - 页面侧：`ask<T>(message, send?)`、`bridgeStatus(connect: boolean, force?: boolean, send?)`、`callStacker(call: StackerCall, payload: unknown, send?)`、`requestFlush(send?)`；`send` 默认 `chrome.runtime.sendMessage`。

- [ ] **Step 1: 写失败的测试**

`extension/src/lib/bridgeMessages.test.ts`：

```ts
import "fake-indexeddb/auto";
import { describe, expect, it, vi } from "vitest";
import { BridgeError, type Bridge } from "./bridge";
import { ask, createBridgeHandler, isBridgeMessage, type BridgeStatus } from "./bridgeMessages";
import { openDb } from "./db";

let n = 0;
function fakeBridge(up = false, fail?: BridgeError) {
  let connected = up;
  const bridge: Bridge = {
    call: vi.fn(async (type: string) => { if (fail) throw fail; connected = true; return { echo: type }; }),
    connected: () => connected,
    lastError: () => (connected ? "" : "host not found"),
    reset: vi.fn(),
  };
  return bridge;
}
function handler(bridge: Bridge) {
  const flushSoon = vi.fn();
  const name = `bm-${n++}`;
  const handle = createBridgeHandler({ bridge, openDb: () => openDb(name), flushSoon, lastSyncAt: () => 42 });
  return { handle, flushSoon };
}

describe("bridge messages", () => {
  it("accepts only the messages pages may send", () => {
    expect(isBridgeMessage({ type: "bridge-status", connect: true, force: false })).toBe(true);
    expect(isBridgeMessage({ type: "bridge-flush" })).toBe(true);
    expect(isBridgeMessage({ type: "bridge-call", call: "saveExport", payload: {} })).toBe(true);
    expect(isBridgeMessage({ type: "bridge-call", call: "syncAccounts", payload: {} })).toBe(false);
    expect(isBridgeMessage({ type: "save-excerpt" })).toBe(false);
    expect(isBridgeMessage(null)).toBe(false);
  });

  it("connects on a status check and flushes what is pending", async () => {
    const bridge = fakeBridge();
    const { handle, flushSoon } = handler(bridge);
    const reply = await handle({ type: "bridge-status", connect: true, force: true });
    expect(bridge.reset).toHaveBeenCalled();
    expect(bridge.call).toHaveBeenCalledWith("status");
    // A fresh database holds the upgrade's "all" entry.
    expect(reply).toEqual({ ok: true, value: { connected: true, pending: 1, lastSyncAt: 42, error: "" } satisfies BridgeStatus });
    expect(flushSoon).toHaveBeenCalledWith(0);
  });

  it("reports standalone without trying when not asked to connect", async () => {
    const bridge = fakeBridge();
    const { handle, flushSoon } = handler(bridge);
    const reply = await handle({ type: "bridge-status", connect: false, force: false });
    expect(bridge.call).not.toHaveBeenCalled();
    expect(reply).toMatchObject({ ok: true, value: { connected: false, error: "host not found" } });
    expect(flushSoon).not.toHaveBeenCalled();
  });

  it("passes calls through and maps bridge errors to their code", async () => {
    const ok = handler(fakeBridge(true));
    expect(await ok.handle({ type: "bridge-call", call: "pullBackup", payload: { section: "accounts", offset: 0 } }))
      .toEqual({ ok: true, value: { echo: "pullBackup" } });
    const failing = handler(fakeBridge(true, new BridgeError("E_PATH")));
    expect(await failing.handle({ type: "bridge-call", call: "saveExport", payload: {} })).toEqual({ ok: false, error: "E_PATH" });
    expect(await ok.handle({ type: "bridge-flush" })).toEqual({ ok: true, value: null });
    expect(ok.flushSoon).toHaveBeenCalledWith(500);
  });

  it("turns a reply into a value or a BridgeError on the page side", async () => {
    expect(await ask({ type: "bridge-flush" }, async () => ({ ok: true, value: 7 }))).toBe(7);
    await expect(ask({ type: "bridge-flush" }, async () => ({ ok: false, error: "E_TIMEOUT" }))).rejects.toMatchObject({ code: "E_TIMEOUT" });
    await expect(ask({ type: "bridge-flush" }, async () => undefined)).rejects.toMatchObject({ code: "E_NOT_CONNECTED" });
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/lib/bridgeMessages.test.ts`
Expected: FAIL（模块不存在）。

- [ ] **Step 3: 实现消息模块**

`extension/src/lib/bridgeMessages.ts`：

```ts
import { BridgeError, type Bridge } from "./bridge";
import { outboxCount, type Db } from "./db";

export interface BridgeStatus { connected: boolean; pending: number; lastSyncAt: number | null; error: string }
/** The only Stacker requests pages may make; syncing stays inside the background. */
export type StackerCall = "saveExport" | "pullBackup";
export type BridgeMessage =
  | { type: "bridge-status"; connect: boolean; force: boolean }
  | { type: "bridge-flush" }
  | { type: "bridge-call"; call: StackerCall; payload: unknown };
export type BridgeReply = { ok: true; value: unknown } | { ok: false; error: string };

const CALLS: StackerCall[] = ["saveExport", "pullBackup"];

export function isBridgeMessage(m: unknown): m is BridgeMessage {
  const x = m as { type?: unknown; call?: unknown } | null;
  if (!x) return false;
  if (x.type === "bridge-status" || x.type === "bridge-flush") return true;
  return x.type === "bridge-call" && CALLS.includes(x.call as StackerCall);
}

export interface HandlerDeps {
  bridge: Bridge;
  openDb: () => Promise<Db>;
  flushSoon: (delayMs: number) => void;
  lastSyncAt: () => number | null;
}

/** Background side: answers the manage page and popup. */
export function createBridgeHandler(deps: HandlerDeps) {
  return async (m: BridgeMessage): Promise<BridgeReply> => {
    try {
      if (m.type === "bridge-flush") {
        deps.flushSoon(500);
        return { ok: true, value: null };
      }
      if (m.type === "bridge-call") return { ok: true, value: await deps.bridge.call(m.call, m.payload) };
      if (m.force) deps.bridge.reset();
      if (m.connect && !deps.bridge.connected()) {
        try { await deps.bridge.call("status"); } catch { /* reported below as not connected */ }
      }
      const pending = await outboxCount(await deps.openDb());
      if (pending && deps.bridge.connected()) deps.flushSoon(0);
      const status: BridgeStatus = { connected: deps.bridge.connected(), pending, lastSyncAt: deps.lastSyncAt(), error: deps.bridge.lastError() };
      return { ok: true, value: status };
    } catch (e) {
      return { ok: false, error: e instanceof BridgeError ? e.code : e instanceof Error ? e.message : String(e) };
    }
  };
}

type Send = (message: BridgeMessage) => Promise<unknown>;
const viaRuntime: Send = (message) => chrome.runtime.sendMessage(message);

/** Page side: one message to the background, its reply as a value or a BridgeError. */
export async function ask<T>(message: BridgeMessage, send: Send = viaRuntime): Promise<T> {
  const reply = (await send(message)) as BridgeReply | undefined;
  if (!reply) throw new BridgeError("E_NOT_CONNECTED");
  if (!reply.ok) throw new BridgeError(reply.error);
  return reply.value as T;
}

export const bridgeStatus = (connect: boolean, force = false, send?: Send) =>
  ask<BridgeStatus>({ type: "bridge-status", connect, force }, send);
export const callStacker = (call: StackerCall, payload: unknown, send?: Send) =>
  ask<unknown>({ type: "bridge-call", call, payload }, send);
export function requestFlush(send: Send = viaRuntime): void {
  void send({ type: "bridge-flush" }).catch(() => {});
}
```

- [ ] **Step 4: 接入后台与页面**

`extension/src/background.ts`：import 区改为

```ts
import { connectChrome, createBridge } from "./lib/bridge";
import { createBridgeHandler, isBridgeMessage } from "./lib/bridgeMessages";
import { addExcerpt, enqueueAll, onOutboxChange, openDb, outboxCount } from "./lib/db";
import { LAST_TAB_KEY, shouldRemember, siteTabOf } from "./lib/lastTab";
import { isValidSaveExcerpt } from "./lib/saveExcerpt";
import { flush } from "./lib/sync";
```

文件末尾追加：

```ts
// --- Stacker bridge: the background owns the native port; pages ask through messages. ---
const bridge = createBridge(connectChrome, {
  version: chrome.runtime.getManifest().version,
  onConnected: (hello) => {
    // Stacker's copy is empty (new install or wiped data): send everything once.
    void openDb().then(async (db) => {
      if (hello.counts.conversations === 0 && (await db.count("conversations")) > 0) await enqueueAll(db, Date.now());
    });
  },
});
let lastSyncAt: number | null = null;
let flushing = false;
let flushAgain = false;
let flushTimer: ReturnType<typeof setTimeout> | undefined;

async function runFlush() {
  if (flushing) { flushAgain = true; return; }
  flushing = true;
  try {
    const db = await openDb();
    if (await outboxCount(db)) {
      await flush(db, bridge.call);
      lastSyncAt = Date.now();
    }
  } catch {
    // Not connected or Stacker refused: the changes stay queued for the next try.
  } finally {
    flushing = false;
    if (flushAgain) { flushAgain = false; flushSoon(500); }
  }
}

function flushSoon(delayMs: number) {
  clearTimeout(flushTimer);
  flushTimer = setTimeout(() => void runFlush(), delayMs);
}

onOutboxChange(() => flushSoon(500));
flushSoon(1000);

const handleBridge = createBridgeHandler({ bridge, openDb, flushSoon, lastSyncAt: () => lastSyncAt });
chrome.runtime.onMessage.addListener((message: unknown, sender, reply) => {
  if (!isBridgeMessage(message)) return false;
  if (sender.id !== chrome.runtime.id) {
    reply({ ok: false, error: "E_REQUEST" });
    return true;
  }
  void handleBridge(message).then(reply);
  return true;
});
```

`extension/src/ui/manage/main.tsx` 与 `extension/src/ui/popup/main.tsx`：各在 import 区加

```ts
import { requestFlush } from "../../lib/bridgeMessages";
import { onOutboxChange } from "../../lib/db";
```

并在 `createRoot(…)` 之前加：

```ts
// Local changes made on this page reach Stacker through the background.
onOutboxChange(() => requestFlush());
```

- [ ] **Step 5: 运行，确认通过**

Run: `npx vitest run extension/src`、`npm run typecheck`、`npm run lint`、`npm run ext:build`
Expected: 测试与检查通过；构建成功，`extension/dist/manifest.json` 含 `nativeMessaging`。

- [ ] **Step 6: 提交**

```bash
git add extension/src/lib/bridgeMessages.ts extension/src/lib/bridgeMessages.test.ts extension/src/background.ts extension/src/ui/manage/main.tsx extension/src/ui/popup/main.tsx
git commit -m "feat(extension): background syncs the outbox to Stacker when connected" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 17: 管理页：同步状态、从 Stacker 恢复、导出进 Stacker

**Files:**
- Create: `extension/src/lib/restore.ts`, `extension/src/lib/restore.test.ts`, `extension/src/lib/save.ts`, `extension/src/lib/save.test.ts`, `extension/src/ui/manage/SyncStatus.tsx`, `extension/src/ui/manage/SyncStatus.test.tsx`
- Modify: `extension/src/ui/manage/App.tsx`, `extension/src/ui/errors.ts`, `extension/src/i18n.ts`

**Interfaces:**
- Consumes: Task 13 `applyBackup`、`Backup`、`RestoreCounts`；Task 15 `BridgeError`；Task 16 `bridgeStatus`、`callStacker`、`BridgeStatus`、`StackerCall`；G1 `saveFile`（`lib/download.ts`）。
- Produces:
  - `extension/src/lib/restore.ts`：`type Pull = (request: { section: keyof Backup; offset: number }) => Promise<unknown>`；`pullSection(pull, section): Promise<unknown[]>`；`restoreFromStacker(db, pull): Promise<RestoreCounts>`
  - `extension/src/lib/save.ts`：`type SaveFn = (path: string, text: string, mime: string) => Promise<void>`；`STACKER_CHUNK_CHARS = 250_000`；`chunkText(text: string, size?: number): string[]`；`stackerSaver(call?: (call: StackerCall, payload: unknown) => Promise<unknown>, size?: number): SaveFn`；`pickSaver(connected: () => Promise<boolean>, call?, download?: SaveFn): Promise<{ save: SaveFn; where: "stacker" | "downloads" }>`
  - `SyncStatus({ status, busy, onReconnect, onRestore })`

- [ ] **Step 1: 写失败的测试**

`extension/src/lib/restore.test.ts`：

```ts
import "fake-indexeddb/auto";
import { describe, expect, it, vi } from "vitest";
import { getConversation, openDb, outboxCount, readOutbox, dropOutbox } from "./db";
import { restoreFromStacker } from "./restore";

describe("restoreFromStacker", () => {
  it("pages through every section and applies what Stacker has", async () => {
    const db = await openDb("restore-1");
    await dropOutbox(db, (await readOutbox(db, 100)).map((e) => e.seq!));
    const conversation = (id: string) => ({
      key: `chatgpt:${id}`, site: "chatgpt", account: "chatgpt:u", id, title: id, createdAt: 1, updatedAt: 2, archived: false,
      removedAt: null, listedAt: 1, folderId: null, tags: [], favorite: true, note: "", localUpdatedAt: 5,
    });
    const sections: Record<string, unknown[]> = {
      accounts: [{ key: "chatgpt:u", site: "chatgpt", remoteId: "u", name: "Ada", alias: "Work", lastSeen: 1, localUpdatedAt: 5 }],
      folders: [],
      conversations: [conversation("a"), conversation("b"), conversation("c")],
      excerpts: [],
    };
    // One item per page, so paging is exercised.
    const pull = vi.fn(async ({ section, offset }: { section: string; offset: number }) => ({
      items: sections[section].slice(offset, offset + 1),
      next: offset + 1 < sections[section].length ? offset + 1 : null,
    }));
    const counts = await restoreFromStacker(db, pull);
    expect(counts).toEqual({ accounts: 1, folders: 0, conversations: 3, excerpts: 0 });
    expect(pull).toHaveBeenCalledTimes(1 + 1 + 3 + 1);
    expect((await getConversation(db, "chatgpt:c"))?.favorite).toBe(true);
    expect(await outboxCount(db)).toBe(0);
  });

  it("stops when a page does not move forward", async () => {
    const db = await openDb("restore-2");
    const pull = vi.fn(async () => ({ items: [], next: 0 }));
    await expect(restoreFromStacker(db, pull)).resolves.toEqual({ accounts: 0, folders: 0, conversations: 0, excerpts: 0 });
    expect(pull).toHaveBeenCalledTimes(4);
  });
});
```

`extension/src/lib/save.test.ts`：

```ts
import { describe, expect, it, vi } from "vitest";
import { chunkText, pickSaver, stackerSaver, type SaveFn } from "./save";

type StackerFn = (call: "saveExport" | "pullBackup", payload: unknown) => Promise<unknown>;

describe("save", () => {
  it("never splits a surrogate pair", () => {
    const text = `${"a".repeat(9)}😀b`;
    const parts = chunkText(text, 10);
    expect(parts.join("")).toBe(text);
    expect(parts[0]).toBe("a".repeat(9));
    expect(chunkText("")).toEqual([""]);
  });

  it("writes the first piece, then appends to the path Stacker chose", async () => {
    const call = vi.fn<StackerFn>(async () => ({ path: "chatgpt/a (1).md", fullPath: "X" }));
    await stackerSaver(call, 4)("chatgpt/a.md", "abcdefghij", "text/markdown");
    expect(call.mock.calls).toEqual([
      ["saveExport", { path: "chatgpt/a.md", text: "abcd", append: false }],
      ["saveExport", { path: "chatgpt/a (1).md", text: "efgh", append: true }],
      ["saveExport", { path: "chatgpt/a (1).md", text: "ij", append: true }],
    ]);
  });

  it("uses Stacker when connected and the downloads folder otherwise", async () => {
    const download = vi.fn<SaveFn>(async () => {});
    const call = vi.fn<StackerFn>(async () => ({ path: "a.md" }));
    const offline = await pickSaver(async () => false, call, download);
    expect(offline.where).toBe("downloads");
    await offline.save("a.md", "x", "text/markdown");
    expect(download).toHaveBeenCalledWith("a.md", "x", "text/markdown");
    const online = await pickSaver(async () => true, call, download);
    expect(online.where).toBe("stacker");
    await online.save("a.md", "x", "text/markdown");
    expect(call).toHaveBeenCalledTimes(1);
  });
});
```

`extension/src/ui/manage/SyncStatus.test.tsx`：

```tsx
// @vitest-environment jsdom
Object.defineProperty(navigator, "language", { value: "zh-CN", configurable: true });
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import { SyncStatus } from "./SyncStatus";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

function render(node: ReactNode) {
  const host = document.createElement("div");
  document.body.append(host);
  act(() => createRoot(host).render(node));
  return host;
}

describe("SyncStatus", () => {
  it("shows pending changes when connected and offers restore", () => {
    const onRestore = vi.fn();
    const host = render(<SyncStatus status={{ connected: true, pending: 3, lastSyncAt: null, error: "" }} busy={false} onReconnect={() => {}} onRestore={onRestore} />);
    expect(host.textContent).toContain("已连接 Stacker · 待同步 3 项");
    act(() => [...host.querySelectorAll("button")].find((b) => b.textContent === "从 Stacker 恢复")!.click());
    expect(onRestore).toHaveBeenCalled();
  });

  it("offers to reconnect when Stacker is not available", () => {
    const onReconnect = vi.fn();
    const host = render(<SyncStatus status={{ connected: false, pending: 2, lastSyncAt: null, error: "host not found" }} busy={false} onReconnect={onReconnect} onRestore={() => {}} />);
    expect(host.textContent).toContain("未连接 Stacker");
    expect(host.textContent).not.toContain("从 Stacker 恢复");
    act(() => host.querySelector("button")!.click());
    expect(onReconnect).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/lib/restore.test.ts extension/src/lib/save.test.ts extension/src/ui/manage/SyncStatus.test.tsx`
Expected: FAIL（模块不存在）。

- [ ] **Step 3: 实现 restore 与 save**

`extension/src/lib/restore.ts`：

```ts
import { applyBackup, type Backup, type Db, type RestoreCounts } from "./db";

export type Pull = (request: { section: keyof Backup; offset: number }) => Promise<unknown>;
interface PullPage { items: unknown[]; next: number | null }

/** Every item of one section, page by page; stops if Stacker's paging does not move forward. */
export async function pullSection(pull: Pull, section: keyof Backup): Promise<unknown[]> {
  const out: unknown[] = [];
  let offset = 0;
  for (;;) {
    const page = (await pull({ section, offset })) as PullPage;
    out.push(...page.items);
    if (page.next === null || page.next <= offset) return out;
    offset = page.next;
  }
}

/** 「从 Stacker 恢复」: brings back aliases, folders, conversations' local fields and excerpts. */
export async function restoreFromStacker(db: Db, pull: Pull): Promise<RestoreCounts> {
  const backup = {
    accounts: await pullSection(pull, "accounts"),
    folders: await pullSection(pull, "folders"),
    conversations: await pullSection(pull, "conversations"),
    excerpts: await pullSection(pull, "excerpts"),
  } as Backup;
  return applyBackup(db, backup);
}
```

`extension/src/lib/save.ts`：

```ts
import { callStacker, type StackerCall } from "./bridgeMessages";
import { saveFile } from "./download";

export type SaveFn = (path: string, text: string, mime: string) => Promise<void>;
type StackerFn = (call: StackerCall, payload: unknown) => Promise<unknown>;

/** Characters per saveExport message; 250k CJK characters are ~750 KB, under the 1 MB limit. */
export const STACKER_CHUNK_CHARS = 250_000;

/** Splits text into pieces without cutting a surrogate pair (that would not survive JSON on the Rust side). */
export function chunkText(text: string, size = STACKER_CHUNK_CHARS): string[] {
  if (!text) return [""];
  const out: string[] = [];
  let start = 0;
  while (start < text.length) {
    let end = Math.min(start + size, text.length);
    const last = text.charCodeAt(end - 1);
    if (end < text.length && last >= 0xd800 && last <= 0xdbff) end--;
    out.push(text.slice(start, end));
    start = end;
  }
  return out;
}

/** Saves into Stacker's export folder: the first piece picks the file, later pieces append to it. */
export function stackerSaver(call: StackerFn = callStacker, size = STACKER_CHUNK_CHARS): SaveFn {
  return async (path, text) => {
    const [first, ...rest] = chunkText(text, size);
    const saved = (await call("saveExport", { path, text: first, append: false })) as { path: string };
    for (const part of rest) await call("saveExport", { path: saved.path, text: part, append: true });
  };
}

/** Stacker's export folder when connected; the browser's downloads folder otherwise. */
export async function pickSaver(connected: () => Promise<boolean>, call: StackerFn = callStacker, download: SaveFn = saveFile): Promise<{ save: SaveFn; where: "stacker" | "downloads" }> {
  return (await connected()) ? { save: stackerSaver(call), where: "stacker" } : { save: download, where: "downloads" };
}
```

- [ ] **Step 4: 实现状态条与管理页接线**

`extension/src/ui/manage/SyncStatus.tsx`：

```tsx
import { t } from "../../i18n";
import type { BridgeStatus } from "../../lib/bridgeMessages";

/** 「已连接 Stacker · 待同步 N 项」 or 「未连接 Stacker」, with the matching action. */
export function SyncStatus({ status, busy, onReconnect, onRestore }: {
  status: BridgeStatus | null; busy: boolean; onReconnect: () => void; onRestore: () => void;
}) {
  if (!status?.connected) {
    return <span className="mut" title={status?.error || undefined}>
      {t("未连接 Stacker")} <button disabled={busy} onClick={onReconnect}>{t("重新连接")}</button>
    </span>;
  }
  return <span className="mut">
    {t("已连接 Stacker")} · {t("待同步")} {status.pending} {t("项")} <button disabled={busy} onClick={onRestore}>{t("从 Stacker 恢复")}</button>
  </span>;
}
```

`extension/src/ui/manage/App.tsx`：

1. import 区：删除 `import { saveFile } from "../../lib/download";`，加入

```ts
import { bridgeStatus, callStacker, type BridgeStatus } from "../../lib/bridgeMessages";
import { restoreFromStacker } from "../../lib/restore";
import { pickSaver } from "../../lib/save";
import { SyncStatus } from "./SyncStatus";
```

2. 在 `const [broken, setBroken] = useState<BrokenSites>({});` 之后加：

```ts
  const [bridge, setBridge] = useState<BridgeStatus | null>(null);
```

3. 在 `useEffect(() => { void brokenStore.all().then(setBroken); }, []);` 之后加：

```ts
  // Poll the connection; connecting is cheap when Stacker is registered and skipped for 30 s after a failure.
  useEffect(() => {
    let alive = true;
    const poll = () => void bridgeStatus(true).then((s) => { if (alive) setBridge(s); }, () => { if (alive) setBridge(null); });
    poll();
    const timer = setInterval(poll, 5000);
    return () => { alive = false; clearInterval(timer); };
  }, []);
  const connectedNow = async () => (await bridgeStatus(true).catch(() => null))?.connected ?? false;
```

4. `exportChosen` 整体替换为：

```ts
  const exportChosen = (mode: ExportMode) => guarded(t("正在导出"), async () => {
    const pacer = createPacer();
    const { save, where } = await pickSaver(connectedNow);
    for (const c of chosen) {
      let body = bodyIsFresh(c) ? await getBody(db!, c.key) : undefined;
      if (!body) {
        const fresh = await withPacing(pacer, () => api.read(c.site, c.id));
        await putBody(db!, c.key, fresh, Date.now());
        body = { ...fresh, key: c.key };
      }
      await save(exportFileName(c, "md"), toMarkdown(c, aliasOf(c.account), body, mode, urlOf(c)), "text/markdown");
      if (mode === "full") await save(exportFileName(c, "json"), JSON.stringify(body, null, 2), "application/json");
    }
    setMessage({
      text: where === "stacker"
        ? `${t("已导出")} ${chosen.length} ${t("条到 Stacker 的导出目录")}`
        : `${t("已导出")} ${chosen.length} ${t("条到下载目录的「Stacker 网页对话」文件夹")}`,
      kind: "info",
    });
  });

  const restore = () => {
    if (!db || !confirm(t("从 Stacker 恢复账号备注名、文件夹、标签、收藏、备注和摘录？这台浏览器里较新的修改会保留。"))) return;
    void guarded(t("正在从 Stacker 恢复"), async () => {
      const n = await restoreFromStacker(db, (request) => callStacker("pullBackup", request));
      setMessage({ text: `${t("已恢复")}：${t("账号")} ${n.accounts}，${t("文件夹")} ${n.folders}，${t("对话")} ${n.conversations}，${t("摘录")} ${n.excerpts}`, kind: "info" });
    });
  };
```

5. 顶栏：在 `{busy && <span className="mut">{busy}…</span>}` 之前加：

```tsx
      <SyncStatus status={bridge} busy={!!busy}
        onReconnect={() => void bridgeStatus(true, true).then(setBridge, () => setBridge(null))}
        onRestore={restore} />
```

6. 删除任务：`onRun` 里 `runDeleteJob(runItems, mode, { api, db, pacer: createPacer(), now: Date.now, save: saveFile, aliasOf }, signal, onProgress)` 改为：

```ts
        const { save } = await pickSaver(connectedNow);
        const results = await runDeleteJob(runItems, mode, { api, db, pacer: createPacer(), now: Date.now, save, aliasOf }, signal, onProgress);
```

`extension/src/ui/errors.ts`：import 加 `import { BridgeError } from "../lib/bridge";`；`ERROR_TEXT` 末尾加：

```ts
  E_NOT_CONNECTED: "未连接 Stacker",
  E_TIMEOUT: "Stacker 没有响应，请稍后再试",
  E_PATH: "导出文件名无效",
  E_STORAGE: "Stacker 无法写入它的数据目录",
  E_REQUEST: "Stacker 拒绝了这个请求",
```

`errorText` 的第一行改为：

```ts
  const code = e instanceof SiteError || e instanceof BridgeError ? e.code : typeof e === "string" ? e : "";
```

`extension/src/i18n.ts` 的 `EN` 里（`// ui/errors.ts` 段之前）加：

```ts
  // manage/SyncStatus.tsx, manage/App.tsx (Stacker)
  "未连接 Stacker": "Not connected to Stacker",
  "重新连接": "Reconnect",
  "已连接 Stacker": "Connected to Stacker",
  "待同步": "pending:",
  "项": "item(s)",
  "从 Stacker 恢复": "Restore from Stacker",
  "正在从 Stacker 恢复": "Restoring from Stacker",
  "从 Stacker 恢复账号备注名、文件夹、标签、收藏、备注和摘录？这台浏览器里较新的修改会保留。": "Restore account aliases, folders, tags, favorites, notes and excerpts from Stacker? Newer changes in this browser are kept.",
  "已恢复": "Restored",
  "账号": "Accounts",
  "文件夹": "Folders",
  "对话": "Conversations",
  "条到 Stacker 的导出目录": "item(s) to Stacker's export folder",
  "Stacker 没有响应，请稍后再试": "Stacker did not respond; try again later",
  "导出文件名无效": "Invalid export file name",
  "Stacker 无法写入它的数据目录": "Stacker cannot write to its data folder",
  "Stacker 拒绝了这个请求": "Stacker refused the request",
```

（`摘录`、`已导出` 已有英文，不要重复。）

- [ ] **Step 5: 运行，确认通过**

Run: `npx vitest run extension/src`、`npm run typecheck`、`npm run lint`、`npm run ext:build`
Expected: 全部通过（`i18n.test.ts` 确认每个 `t("…")` 都有英文）。

- [ ] **Step 6: 提交**

```bash
git add extension/src/lib/restore.ts extension/src/lib/restore.test.ts extension/src/lib/save.ts extension/src/lib/save.test.ts extension/src/ui/manage/SyncStatus.tsx extension/src/ui/manage/SyncStatus.test.tsx extension/src/ui/manage/App.tsx extension/src/ui/errors.ts extension/src/i18n.ts
git commit -m "feat(extension): sync status, restore from Stacker, exports into Stacker" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---
### Task 18: 文档

**Files:**
- Modify: `extension/README.md`, `docs/sessions.md`, `docs/superpowers/specs/2026-09-19-web-chat-extension-design.md`

**Interfaces:**
- Consumes: 前面各任务的行为（路径、按钮名、状态文字、错误处理）。
- Produces: 用户文档与设计文档与实现一致。

- [ ] **Step 1: 插件说明**

`extension/README.md`：
- 「原则」第一条改为：`- 数据只存在本机：浏览器的 IndexedDB；连接本机 Stacker 后再同步一份到 Stacker 的数据目录。不上传到任何服务器。`
- 「导出」一节末尾加一句：`连接 Stacker 后，导出文件改存到 Stacker 的导出目录 %LOCALAPPDATA%\Stacker\stable\conversations\exports\web\<站点>\（同名时自动加「 (1)」）；未连接时照旧存到下载目录。`
- 在「站点接口变化时的表现」之前新增一节：

```markdown
## 连接 Stacker

插件可以完全单独使用。本机装有 Stacker 时，可以把两者连接起来：

1. 在 Stacker 打开「会话数据 → 设置 → 浏览器插件」，看到插件文件夹位置（安装版 Stacker 自带插件，位于程序目录下的 `extension`）。
2. 用这个文件夹按上面的「安装」步骤加载插件（已经加载过 `extension/dist` 的也可以继续用）。
3. 在同一处点对应浏览器的「连接」。Stacker 会在当前用户的注册表写入 `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.stacker.webchat`（Edge 为 `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.stacker.webchat`），并在数据目录保存登记文件 `native-messaging\com.stacker.webchat.json`。
4. 回到 `chrome://extensions`（或 `edge://extensions`）点插件的「重新加载」。

连接后：

- 管理页顶部显示「已连接 Stacker · 待同步 N 项」；连不上时显示「未连接 Stacker」，插件照常单独工作，可点「重新连接」重试。
- 每次改动（刷新列表、读取正文、文件夹、标签、收藏、备注、账号备注名、摘录、删除）都先记入插件的待同步队列，连接时自动小批量推送给 Stacker；推送失败的留在队列里下次再推，不会丢。
- 同步的内容：账号内部编号与备注名、对话列表与整理字段、已读取的正文、文件夹、摘录。不同步任何凭证，不保存邮箱。
- Stacker「会话数据 → 网页对话」可以查看、搜索（含正文）这些对话，并用本机智能体生成摘要。
- 「从 Stacker 恢复」：重装浏览器或插件后，一键找回账号备注名、文件夹、标签、收藏、备注和摘录；这台浏览器里较新的修改会保留。
- 浏览器只在需要同步时启动 Stacker 的后台程序（`stacker.exe`，不打开窗口），空闲 2 分钟后自动断开。
- 在 Stacker 点「断开」会删除注册表项；两个浏览器都断开后删除登记文件。

开发时可以不经浏览器直接检查桥接程序：`node scripts/webchat-bridge-probe.mjs src-tauri/target/debug/stacker.exe`（只读）；加 `--write` 并把环境变量 `STACKER_WEBCHAT_DIR` 指向一个临时目录，可以把示例数据写到那个目录里检查。浏览器正在使用调试版 `stacker.exe` 时重新编译可能因文件被占用而失败，关掉管理页等 2 分钟或在 `chrome://extensions` 停用插件即可。
```

- [ ] **Step 2: 会话数据说明**

`docs/sessions.md`：在「## 迁移到其他盘」之前新增两节：

```markdown
## 网页对话

「网页对话」标签显示从「Stacker 网页对话」浏览器插件同步来的 ChatGPT、Claude 等网站对话（连接方法见下一节）。

- 列表按最后更新时间排序，每页 100 条；可按站点、账号筛选，搜索标题、备注、标签和摘要；勾选「搜索正文」时同时搜索已同步的正文（需要逐条解压，较慢）。
- 标注：「网站上已删除」（插件刷新时发现网站上已没有这条对话，或在插件里删除了）、「未存正文」（插件还没读取过正文）、「正文不是最新」（读取正文后网站上又有更新）、「有摘要」。
- 详情显示正文、文件夹、标签、备注和摘要。「生成摘要」先确认将发送的字数和执行者，由本机智能体生成（与会话摘要同一套运行方式和「设置 → 摘要」中的模型与推理强度；执行者设为「同源」时网页对话用 Claude）。正文更新后摘要标记「已过期」。同一时间只生成一个网页对话摘要，可取消。
- Stacker 不会修改或删除网站上的对话；清理在插件里完成。

## 浏览器插件

「设置 → 浏览器插件」：

- 插件文件夹：安装版与免安装版在 `stacker.exe` 所在目录下的 `extension`；开发时为仓库的 `extension\dist`（先运行 `npm run ext:build`）。「打开文件夹」在资源管理器中打开它。
- 安装步骤：在 `chrome://extensions` 或 `edge://extensions` 打开「开发者模式」，「加载已解压的扩展程序」选择插件文件夹，再点下方的「连接」并重新加载插件。
- 「连接」（先确认）在当前用户注册表写入 `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.stacker.webchat` 或 `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.stacker.webchat`，值为登记文件 `%LOCALAPPDATA%\Stacker\<dev|stable>\conversations\native-messaging\com.stacker.webchat.json` 的路径；登记文件指向当前的 `stacker.exe`，只允许固定 ID 的插件连接。「断开」删除注册表项，两个浏览器都断开后删除登记文件。
- 状态：已连接 / 未连接 / 「登记指向其他位置」（例如换了安装位置，或开发版与正式版互相覆盖），后者点「连接」即可更新。
- 最近连接时间、最近同步时间，以及已同步的账号、对话、正文、文件夹、摘录数量。

浏览器以 `stacker.exe chrome-extension://<插件 ID>/` 启动 Stacker 时进入桥接模式：只通过标准输入输出与插件交换数据，不打开窗口，也不会唤起已运行的 Stacker；来源不是固定插件 ID 时立即退出。插件导出的文件存到导出目录下的 `web\<站点>\`，路径最多 4 级、只允许 `.md` 与 `.json`，不能写到导出目录之外。
```

在「## Stacker 保存的数据」一节末尾追加：

```markdown
`%LOCALAPPDATA%\Stacker\<dev|stable>\conversations\webchat.sqlite3`（与 `sessions.sqlite3` 分开）：

- `web_accounts`、`web_conversations`、`web_folders`、`web_excerpts`：插件同步来的账号、对话索引与整理字段、文件夹、摘录，以及网页对话的摘要。
- 合并规则：标题、时间、归档、删除标记以最新一次列表刷新为准；文件夹、标签、收藏、备注、账号备注名以最后修改的一方为准。文件夹与摘录删除后不会被旧数据恢复。
- 正文：`conversations\webchat\bodies\<站点>\<账号>\<对话>.json.gz`；导出：`conversations\exports\web\`；登记文件：`conversations\native-messaging\`。
```

- [ ] **Step 3: 设计文档**

`docs/superpowers/specs/2026-09-19-web-chat-extension-design.md`：
- 第 4 行 `状态：G1 已实现` 改为 `状态：G1、G3 已实现`（若 G2 已合入则写 `G1–G3 已实现`）。
- §2 表格「通信」一行改为 `| 通信 | Chrome 本地消息（Native Messaging），由 Stacker 主程序的桥接模式处理 |`。
- §3.2 整节替换为：

```markdown
### 3.2 桥接（Stacker 主程序的桥接模式）

- 不另做二进制：浏览器以 `stacker.exe chrome-extension://<插件 ID>/` 启动时，Stacker 进入桥接模式，只通过标准输入输出（4 字节小端长度 + JSON，单条 ≤ 1 MB）与插件通信，不启动界面、不打开窗口。
- 本地消息主机名 `com.stacker.webchat`；登记清单放在 Stacker 数据目录，`allowed_origins` 只含固定插件 ID；清单的 `path` 是当前的 `stacker.exe`。
- 直接读写 Stacker 数据（`webchat.sqlite3`，WAL 模式，与主程序并发安全）。
```

- §4.2 第一条末尾加：`网页对话单独存放在 webchat.sqlite3。`；§4.3「合并」一行改为：`- 合并：站点字段（标题、时间、归档、删除）以较新的列表刷新为准；整理字段（文件夹、标签、收藏、备注、备注名）按记录比较修改时间，较新者生效。`

- [ ] **Step 4: 检查并提交**

Run: `npm run check:i18n`（文档不参与，但确认前面任务仍通过）

```bash
git add extension/README.md docs/sessions.md docs/superpowers/specs/2026-09-19-web-chat-extension-design.md
git commit -m "docs: connecting the browser extension to Stacker" -m "Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 19: 实机验证（仅由协调者执行，不交给子代理）

控制者不能在浏览器里重新加载插件，因此：桥接程序用命令行探针像浏览器一样驱动；插件端的行为由 Task 13–17 的单元测试覆盖；最后请用户重新加载插件做一次端到端查看。用户已同意由控制者写入 Chrome 登记并保留。

- [ ] **Step 1: 全量检查**

Run:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
npm run typecheck
npm run lint
npx vitest run
npm run check:i18n
npm run ext:build
cargo build --manifest-path src-tauri/Cargo.toml
```

Expected: 全部通过；`src-tauri/target/debug/stacker.exe` 已是最新。

- [ ] **Step 2: 用临时目录跑写入探针**

```powershell
$env:STACKER_WEBCHAT_DIR = Join-Path $env:TEMP "stacker-webchat-probe"
Remove-Item $env:STACKER_WEBCHAT_DIR -Recurse -Force -ErrorAction SilentlyContinue
node scripts/webchat-bridge-probe.mjs src-tauri/target/debug/stacker.exe --write
Get-ChildItem -Recurse $env:STACKER_WEBCHAT_DIR | Select-Object FullName, Length
Remove-Item Env:STACKER_WEBCHAT_DIR
```

Expected: 所有行 `ok`，脚本退出码 0；目录下有 `webchat.sqlite3`、`webchat\bodies\chatgpt\probe\probe-1.json.gz`、`exports\web\chatgpt\probe.md`（内容为 `# Probe` 加 `more`）；过程中没有任何窗口出现。

- [ ] **Step 3: 对真实（dev）数据目录跑只读探针**

```powershell
node scripts/webchat-bridge-probe.mjs src-tauri/target/debug/stacker.exe
```

Expected: `hello`、四个 `pullBackup`、`status` 都 `ok`（数据可能为空），退出码 0；dev 数据目录下出现 `webchat.sqlite3`。

- [ ] **Step 4: 写入并保留 Chrome 登记**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib live_register_chrome -- --ignored --nocapture
reg query "HKCU\Software\Google\Chrome\NativeMessagingHosts\com.stacker.webchat" /ve
Get-Content "$env:LOCALAPPDATA\Stacker\dev\conversations\native-messaging\com.stacker.webchat.json"
```

Expected: 测试通过并打印 Chrome 为 `Connected`；注册表默认值为登记文件路径；登记文件的 `path` 为仓库的 `src-tauri\target\debug\stacker.exe`，`allowed_origins` 为 `["chrome-extension://fdfhikmcjcpbihmbnlknbcfghkkopcjl/"]`（`extension/EXTENSION_ID` 的值）。不执行断开，登记保留。

- [ ] **Step 5: 请用户做端到端查看并汇报**

告诉用户：
1. 在 `chrome://extensions` 点「Stacker 网页对话」的「重新加载」（新增了 `nativeMessaging` 权限，插件目录 `extension/dist` 已在 Step 1 重新构建）。
2. 打开插件管理页：顶部应显示「已连接 Stacker · 待同步 N 项」，N 在几秒内降到 0（v2 升级会把已有数据全部推送一次）。
3. 在 Stacker 开发版（`npm run tauri dev`，与登记同一个 `target\debug\stacker.exe` 和 dev 数据目录）打开「会话数据 → 网页对话」，应看到插件里的对话；「设置 → 浏览器插件」中 Chrome 显示「已连接」，有最近连接与同步时间。
4. 注意：浏览器在使用调试版 `stacker.exe` 时重新编译可能因文件占用失败，关掉管理页等 2 分钟即可。

汇报：各检查结果、探针输出摘要、登记的位置，以及仍需用户在浏览器里确认的项目。

---

## Self-Review

- **Spec 覆盖（G3 范围与裁定）**：桥接模式与不开窗口（Task 7，入口在单实例插件之前）；分帧与 1 MiB 上限（Task 1、6）；协议全部类型 `hello`/`syncAccounts`/`syncConversations`(≤200)/`syncBody`(分段)/`syncFolders`/`syncExcerpts`/`removeRecords`/`pullBackup`/`saveExport`/`status`（Task 2、6）；`webchat.sqlite3` 独立库与四张表、合并规则（Task 3）；gzip 正文路径（Task 4）；导出目录与防越界（Task 5）；主机名、清单、`allowed_origins`、Chrome/Edge 的 HKCU 登记与「断开」删除（Task 8）；「网页对话」列表、按站点账号筛选、标题与正文搜索、查看、摘要沿用执行器与摘要设置（Task 9、12）；「浏览器插件」设置块：文件夹与打开、安装步骤、连接/断开、状态、最近连接与同步时间、数量（Task 9、11）；插件目录随安装包与免安装版附带、开发时指向 `extension/dist`（Task 8、10）；插件 IndexedDB v2 与 `outbox`、每处修改入队（Task 13）；后台持有端口、按需连接、超时、单独使用（Task 15、16）；分批与 1 MB 限制（Task 14）；管理页状态文字、「从 Stacker 恢复」、连接时导出走 `saveExport`（Task 17）；`nativeMessaging` 权限与测试（Task 13）；文档（Task 18）；实机验证并保留 Chrome 登记（Task 19）。设计文档 §5.2「与本机会话统一搜索」按裁定以独立标签实现，合并搜索不在本期；G4 提炼不在本计划。
- **约束核对**：注册表只在「连接」命令（用户点击并确认）与 Task 19 的忽略测试中写入；Stacker 侧站点 id 全为字符串，前端 `webSiteLabel` 对未知站点原样显示，G2 新站点无需改 Stacker；不读凭证、不存邮箱（同步的账号字段只有内部编号、站点名称、备注名）；Rust 只用 MSRV 1.77.2 可用的 API（`is_some_and`、`is_ok_and`、`[char; N]` 模式、`let … else` 均早于 1.77）。
- **占位扫描**：每个代码步骤都给出完整代码或确切的替换位置；英文对照逐条列出；没有待定内容。
- **类型一致**：Rust `WebConversation`/`WebAccount`/`WebFolder`/`WebExcerpt` 的 camelCase 字段与插件 `sync.ts` 的 `wire*` 输出、`applyBackup` 读取的字段一一对应（`listedAt`、`localUpdatedAt`、`remoteId`、`conversationId`、`pageTitle`）；`BodyChunk` 两侧字段相同（`chunk`、`chunks`、`fetchedAt`）；`pullBackup` 两侧为 `{ section, offset }` → `{ items, next }`；`saveExport` 两侧为 `{ path, text, append }` → `{ path, fullPath }`；前端 `WebChat`、`WebchatStatus`、`WebChatDetail` 与 Rust `WebChatRow`、`WebchatStatus`、`WebChatDetail` 的序列化字段一致；命令名与参数名（`query`、`key`、`browser`、`target`、`settings`、`locale`）在 `api.ts` 与 `#[tauri::command]` 中一致。
