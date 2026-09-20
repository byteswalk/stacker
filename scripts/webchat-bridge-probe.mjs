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

  // The appearance is shared with the app window, so it lives in the real settings.json:
  // read it, flip it, check it came back, then put the original value back.
  const before = (await call("status")).result?.theme;
  const other = before === "light" ? "dark" : "light";
  const set = await call("setTheme", { theme: other });
  const after = (await call("status")).result?.theme;
  report("setTheme changes the shared appearance", set.ok === true && after === other, `${before} -> ${after}`);
  const nonsense = await call("setTheme", { theme: "chartreuse" });
  const fallback = (await call("status")).result?.theme;
  report("an unknown appearance falls back instead of sticking", nonsense.ok === true && ["dark", "light", "system"].includes(fallback), String(fallback));
  await call("setTheme", { theme: before });
  report("the original appearance is restored", (await call("status")).result?.theme === before, String(before));
}
for (const section of ["accounts", "folders", "conversations", "excerpts"]) {
  await expectOk(`pullBackup ${section}`, "pullBackup", { section, offset: 0 });
}
await expectOk("status", "status");
child.stdin.end();
const code = await exited;
report("bridge exits when the pipe closes", code === 0, `exit ${code}`);
process.exitCode = failed ? 1 : 0;
