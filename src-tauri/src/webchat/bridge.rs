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

/// `hello`'s own id, plus room to spare; a well-formed request never needs more, and an
/// id this long could otherwise push `encode_response`'s id-carrying fallback over the limit.
const MAX_ID: usize = 128;

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
            Ok(request) if request.id.len() > MAX_ID => {
                Response::failure(String::new(), "E_REQUEST")
            }
            Ok(request) => ctx.handle(request),
            Err(_) => Response::failure(String::new(), "E_REQUEST"),
        };
        if framing::write_message(output, &encode_response(&response)).is_err() {
            return 1;
        }
    }
}

/// A response that would not fit one native message is replaced by `E_TOO_LARGE`; the
/// fallback always fits, even when the request's own id is what made the first one too big.
pub fn encode_response(response: &Response) -> Vec<u8> {
    let bytes = serde_json::to_vec(response).unwrap_or_default();
    if bytes.len() <= MAX_MESSAGE {
        return bytes;
    }
    let with_id = serde_json::to_vec(&Response::failure(response.id.clone(), "E_TOO_LARGE"))
        .unwrap_or_default();
    if with_id.len() <= MAX_MESSAGE {
        return with_id;
    }
    serde_json::to_vec(&Response::failure(String::new(), "E_TOO_LARGE")).unwrap_or_default()
}

pub struct Context {
    conn: Connection,
    root: PathBuf,
    exports: PathBuf,
    /// Export files this process created; only these may be appended to.
    written: HashSet<PathBuf>,
}

/// Where the vault keeps the index of logins that can be filled.
fn logins_index() -> PathBuf {
    crate::vault::logins::index_path(&crate::vault::default_path())
}

#[derive(serde::Deserialize)]
struct LoginPage {
    url: String,
}

#[derive(serde::Deserialize)]
struct LoginPick {
    url: String,
    user: String,
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
                // 插件跟着这个值走，两边外观保持一致。
                "theme": crate::settings::load().theme,
            })),
            // 插件那边改了外观：写进同一份设置，主窗口核对后跟上。
            "setTheme" => {
                let request: SetTheme = parse(payload)?;
                let theme = crate::settings::set_theme(&request.theme)?;
                Ok(json!({ "theme": theme }))
            }
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
                let page = store::pull(
                    &self.conn,
                    &request.section,
                    request.offset,
                    store::PULL_BUDGET,
                )?;
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
            // 只读：插件看某条对话的提炼结果，提炼本身只在 Stacker 里发起。
            "distillResults" => {
                let lookup: DistillLookup = parse(payload)?;
                if !is_site(&lookup.site) || lookup.id.is_empty() || lookup.id.len() > 200 {
                    return Err("E_REQUEST".into());
                }
                let key = format!("web:{}:{}", lookup.site, lookup.id);
                let items = crate::distill::store::for_source(&self.conn, &key)?;
                Ok(json!({ "items": crate::distill::bridge_items(items) }))
            }
            // 网站登录：只经过 Windows 凭据管理器和本机加密的索引，不碰保管库本身。
            "loginsFor" => {
                let request: LoginPage = parse(payload)?;
                let index = crate::vault::logins::read_index(&logins_index());
                let found = crate::vault::logins::matches(&index, &request.url);
                Ok(json!({
                    "logins": found.iter().map(|item| json!({ "user": item.user, "title": item.title, "host": item.host })).collect::<Vec<_>>(),
                }))
            }
            "loginPassword" => {
                let request: LoginPick = parse(payload)?;
                let index = crate::vault::logins::read_index(&logins_index());
                let password = crate::vault::logins::password_for(
                    &crate::vault::wincred::SystemStore,
                    &index,
                    &request.url,
                    &request.user,
                )
                .ok_or("E_NOT_FOUND")?;
                Ok(json!({ "password": password.as_str() }))
            }
            "loginSave" => {
                let captured: crate::vault::logins::Captured = parse(payload)?;
                if captured.url.len() > 2048
                    || captured.user.len() > 256
                    || captured.password.len() > 512
                    || captured.title.len() > 200
                {
                    return Err("E_REQUEST".into());
                }
                crate::vault::logins::put_inbox(&crate::vault::wincred::SystemStore, &captured)?;
                Ok(json!({ "saved": true }))
            }
            _ => Err("E_UNKNOWN_TYPE".into()),
        }
    }
}

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
            origin_arg(&args(&[
                "stacker.exe",
                "chrome-extension://abc/",
                "--parent-window=0"
            ])),
            Some("chrome-extension://abc/".to_string())
        );
        assert_eq!(origin_arg(&args(&["stacker.exe"])), None);
        assert_eq!(origin_arg(&args(&["stacker.exe", "--autostart"])), None);
    }

    #[test]
    fn a_foreign_origin_is_refused_before_reading_anything() {
        assert_eq!(
            run_stdio("chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"),
            2
        );
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
        assert_eq!(
            out[1],
            json!({"id": "2", "ok": false, "error": "E_UNKNOWN_TYPE"})
        );
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
        assert_eq!(
            (out[1]["id"].as_str(), out[1]["ok"].as_bool()),
            (Some("9"), Some(true))
        );
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
    fn an_overlong_id_is_refused_before_dispatch_and_the_loop_goes_on() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = context(&dir);
        let out = exchange(
            &mut ctx,
            &[
                json!({"id": "x".repeat(MAX_ID + 1), "type": "status"}),
                json!({"id": "9", "type": "status"}),
            ],
        );
        assert_eq!(out[0], json!({"id": "", "ok": false, "error": "E_REQUEST"}));
        assert_eq!(
            (out[1]["id"].as_str(), out[1]["ok"].as_bool()),
            (Some("9"), Some(true))
        );
    }

    #[test]
    fn responses_over_the_limit_become_an_error() {
        let big = Response::success("7".into(), json!("x".repeat(MAX_MESSAGE)));
        let value: Value = serde_json::from_slice(&encode_response(&big)).unwrap();
        assert_eq!(
            value,
            json!({"id": "7", "ok": false, "error": "E_TOO_LARGE"})
        );
    }

    #[test]
    fn the_too_large_fallback_always_fits_even_when_the_id_itself_is_huge() {
        // With MAX_ID enforced before dispatch this cannot happen in practice, but
        // encode_response must still hold the line on its own: the fallback can never
        // grow the message back over the limit just by carrying the same id.
        let huge_id = "x".repeat(MAX_MESSAGE);
        let big = Response::success(huge_id, json!("x".repeat(MAX_MESSAGE)));
        let bytes = encode_response(&big);
        assert!(
            bytes.len() <= MAX_MESSAGE,
            "the fallback response must always fit one frame"
        );
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            value,
            json!({"id": "", "ok": false, "error": "E_TOO_LARGE"})
        );
    }

    #[test]
    fn batches_over_the_limit_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = context(&dir);
        let items: Vec<Value> = (0..=MAX_BATCH)
            .map(|i| json!({"key": format!("chatgpt:u{i}"), "site": "chatgpt", "remoteId": format!("u{i}")}))
            .collect();
        let out = exchange(
            &mut ctx,
            &[json!({"id": "1", "type": "syncAccounts", "payload": {"items": items}})],
        );
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
        let excerpt = |id: &str, text: &str| {
            json!({
                "id": id, "site": "chatgpt", "conversationId": "a", "url": "https://chatgpt.com/c/a",
                "pageTitle": "Trip", "text": text, "note": "", "createdAt": 1, "localUpdatedAt": 1
            })
        };
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
    fn the_extension_can_read_a_conversations_distilled_results() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = context(&dir);
        let source = |key: &str| crate::distill::DistillSource {
            key: key.into(),
            kind: "web".into(),
            title: "Trip plan".into(),
            link: String::new(),
        };
        let long = "y".repeat(crate::distill::BRIDGE_BODY_CHARS + 50);
        for (id, kind, title, body, key) in [
            ("qa-1", "qa", "Where to go", "Kyoto.", "web:chatgpt:a"),
            (
                "skill-1",
                "skill",
                "Plan a trip",
                long.as_str(),
                "web:chatgpt:a",
            ),
            ("qa-2", "qa", "Other chat", "Nope.", "web:chatgpt:b"),
        ] {
            crate::distill::store::insert(
                &ctx.conn,
                &crate::distill::store::DistillResult {
                    id: id.into(),
                    kind: kind.into(),
                    title: title.into(),
                    body: body.into(),
                    sources: vec![source(key)],
                    state: "draft".into(),
                    by: "claude / sonnet / low".into(),
                    folder: String::new(),
                    created_at: 1,
                    updated_at: 2,
                },
            )
            .unwrap();
        }
        let out = exchange(
            &mut ctx,
            &[
                json!({"id": "1", "type": "distillResults", "payload": {"site": "chatgpt", "id": "a"}}),
                json!({"id": "2", "type": "distillResults", "payload": {"site": "chatgpt", "id": "zzz"}}),
                json!({"id": "3", "type": "distillResults", "payload": {"site": "Chat GPT", "id": "a"}}),
                json!({"id": "4", "type": "distillResults", "payload": {"site": "chatgpt", "id": ""}}),
            ],
        );
        let items = out[0]["result"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 2, "only this conversation's results");
        let titles: Vec<&str> = items.iter().map(|i| i["title"].as_str().unwrap()).collect();
        assert!(titles.contains(&"Where to go") && titles.contains(&"Plan a trip"));
        assert!(!titles.contains(&"Other chat"));
        let big = items.iter().find(|i| i["title"] == "Plan a trip").unwrap();
        assert!(
            big["body"].as_str().unwrap().chars().count() <= crate::distill::BRIDGE_BODY_CHARS + 1,
            "the body has a cap so it cannot blow out one frame"
        );
        assert_eq!(big["kind"], "skill");
        assert_eq!(big["state"], "draft");
        assert_eq!(big["sources"], json!(["Trip plan"]));
        assert_eq!(out[1]["result"]["items"], json!([]));
        assert_eq!(out[2]["error"], "E_REQUEST");
        assert_eq!(out[3]["error"], "E_REQUEST");
    }

    #[test]
    fn status_carries_the_appearance_the_extension_follows() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = context(&dir);
        let replies = exchange(&mut ctx, &[json!({"id": "1", "type": "status"})]);
        let theme = replies[0]["result"]["theme"].as_str().unwrap();
        assert!(
            ["dark", "light", "system"].contains(&theme),
            "unexpected theme {theme}"
        );
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
        assert!(dir
            .path()
            .join("exports")
            .join("web")
            .join("chatgpt")
            .join("a.md")
            .is_file());
        assert_eq!(out[1]["error"], "E_PATH");
        assert!(!dir.path().join("exports").join("escape.md").exists());
    }

    #[test]
    fn pull_pages_stay_under_the_budget() {
        let items: Vec<Value> = (0..10)
            .map(|i| json!({"i": i, "pad": "x".repeat(100)}))
            .collect();
        let first = store::page_by_bytes(items.clone(), 0, 300);
        assert_eq!((first.items.len(), first.next), (2, Some(2)));
        let last = store::page_by_bytes(items.clone(), 8, 300);
        assert_eq!((last.items.len(), last.next), (2, None));
        let tiny = store::page_by_bytes(items, 0, 10);
        assert_eq!(
            tiny.items.len(),
            1,
            "one oversized item still moves forward"
        );
    }
}
