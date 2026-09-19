# G1：网页对话插件本体（ChatGPT、Claude）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 一个可独立运行的 Chromium MV3 插件，管理 ChatGPT 与 Claude 网页对话：标题索引、按需读正文、搜索、文件夹与标签、导出 Markdown、三种删除模式、弹出小窗与摘录按钮。

**Architecture:** 插件放在仓库 `extension/`，复用根目录的 Vite / React / TypeScript / Vitest / ESLint。站点适配器是纯函数加可注入的 `fetchJson`，在站点页面的内容脚本里运行（借用页面登录状态，同源请求）；管理页通过 `chrome.tabs.sendMessage` 调用它。数据存 IndexedDB（`idb`）。批量删除等长任务在管理页里运行（不依赖会被回收的 service worker），限速、可中止。

**Tech Stack:** TypeScript 6、React 19、Vite 8、Vitest 3（jsdom、fake-indexeddb）、`idb`、`@types/chrome`、Chrome Manifest V3。

**Spec:** `docs/superpowers/specs/2026-09-19-web-chat-extension-design.md`（本计划只覆盖分期 G1）

## Global Constraints

- 浏览器：Chromium 内核（Chrome、Edge），Manifest V3。
- G1 站点：ChatGPT（`https://chatgpt.com/*`）、Claude（`https://claude.ai/*`）；`host_permissions` 只含这两项，不申请全部网站。
- 插件不保存、不导出任何凭证；ChatGPT 的 access token 只在内容脚本内存中使用；不存账号邮箱。
- 无统计上报；数据只在浏览器本地。
- 正文按需读取：打开、导出、删除前导出、或用户点「读取正文」时才读。
- 删除模式：精简导出后删除（默认）/ 完整备份后删除 / 直接删除（二次确认）；限速逐条（默认间隔 1500 ms），遇限流退避，可中止。
- 只操作当前浏览器已登录账号；任务账号与当前账号不符时跳过并提示。
- 适配器校验返回结构；不符即抛 `E_BROKEN`，停止该站点的任务，绝不在结构不明时删除。
- 数据按「站点 + 账号」隔离；对话主键 `${site}:${id}`，账号主键 `${site}:${remoteId}`。
- 界面文案中文，英文走 `extension/src/i18n.ts` 的对照表；ChatGPT 分支对话只取当前显示分支。
- 导出目录（单独使用）：浏览器下载目录下 `Stacker 网页对话/<站点>/`。
- 固定插件 ID：manifest `key` 固定，ID 记录在 `extension/EXTENSION_ID`（G3 桥接程序使用）。
- 不写回网站：文件夹、标签只在插件内。
- 每个任务结束时 `npm run typecheck`、`npm run lint`、`npx vitest run` 全绿。
- 提交信息以 `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` 结尾。
- 使用 Write/Edit 工具写含反斜杠的内容，不用 bash heredoc。

## 文件结构

```
extension/
  EXTENSION_ID                 固定插件 ID（由 manifest key 计算）
  README.md                    安装（开发者模式加载）与使用说明
  tsconfig.json
  vite.config.ts               管理页、小窗、后台脚本构建
  vite.content.config.ts       内容脚本 IIFE 构建
  manage.html  popup.html
  public/manifest.json
  scripts/extension-id.mjs     由 manifest key 计算 ID
  scripts/zip.mjs              打包 dist 为 zip
  src/
    i18n.ts                    t() 与英文对照
    shared/types.ts            共享类型与错误码
    sites/
      http.ts                  fetchJson、HTTP 状态到错误码
      guards.ts                结构校验
      chatgpt.ts               ChatGPT 适配器
      claude.ts                Claude 适配器
      registry.ts              站点表
      fixtures/                脱敏样本
    content/
      agent.ts                 内容脚本 RPC
      excerpt.ts               摘录按钮
      index.ts                 内容脚本入口
    background.ts              打开管理页、小窗，记住最近的站点标签页，保存摘录
    lib/
      siteClient.ts            管理页调用内容脚本（SiteApi）
      db.ts                    IndexedDB 与仓储函数
      refresh.ts               刷新标题索引
      search.ts                筛选与搜索
      markdown.ts              导出 Markdown
      download.ts              chrome.downloads 保存
      pacer.ts                 限速与退避
      deleteJob.ts             删除任务
    ui/
      style.css
      manage/main.tsx  App.tsx  Filters.tsx  ConversationList.tsx  Detail.tsx  DeleteDialog.tsx
      popup/main.tsx  Popup.tsx
```

根目录改动：`package.json`（脚本与依赖）、`tsconfig.json`（引用 `extension/tsconfig.json`）、`eslint.config.js`（`extension/dist` 忽略）、`.gitignore`（`extension/dist`、`extension/*.zip`）。

---

### Task 1: 插件工程骨架

**Files:**
- Create: `extension/tsconfig.json`, `extension/vite.config.ts`, `extension/vite.content.config.ts`, `extension/manage.html`, `extension/popup.html`, `extension/public/manifest.json`, `extension/scripts/extension-id.mjs`, `extension/EXTENSION_ID`, `extension/src/i18n.ts`, `extension/src/i18n.test.ts`, `extension/src/manifest.test.ts`, `extension/src/ui/manage/main.tsx`, `extension/src/ui/popup/main.tsx`, `extension/src/background.ts`, `extension/src/content/index.ts`
- Modify: `package.json`, `tsconfig.json`, `eslint.config.js`, `.gitignore`

**Interfaces:**
- Produces: `t(text: string): string`（`extension/src/i18n.ts`）；`EN: Record<string,string>`；构建脚本 `npm run ext:build` 输出 `extension/dist/`（`manage.html`、`popup.html`、`background.js`、`content.js`、`manifest.json`）。

- [ ] **Step 1: 安装依赖**

```bash
npm install --save-dev @types/chrome fake-indexeddb
npm install idb
```

- [ ] **Step 2: 生成固定 key 与 ID**

用 Node 一次性生成（私钥不需要保存：开发者模式加载只用公钥决定 ID）：

```bash
node -e "const c=require('crypto');const {publicKey}=c.generateKeyPairSync('rsa',{modulusLength:2048});const der=publicKey.export({type:'spki',format:'der'});console.log(der.toString('base64'))"
```

把输出的 base64 写进 `extension/public/manifest.json` 的 `key`。

`extension/scripts/extension-id.mjs`：

```js
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

/** Chrome's extension ID: first 32 hex digits of SHA-256(public key DER), 0-f mapped to a-p. */
export function extensionId(keyBase64) {
  const hex = createHash("sha256").update(Buffer.from(keyBase64, "base64")).digest("hex").slice(0, 32);
  return [...hex].map((c) => String.fromCharCode(97 + parseInt(c, 16))).join("");
}

if (process.argv[1] && process.argv[1].endsWith("extension-id.mjs")) {
  const manifest = JSON.parse(readFileSync(new URL("../public/manifest.json", import.meta.url), "utf8"));
  console.log(extensionId(manifest.key));
}
```

运行 `node extension/scripts/extension-id.mjs > extension/EXTENSION_ID`。

- [ ] **Step 3: manifest**

`extension/public/manifest.json`（`key` 为 Step 2 的值）：

```json
{
  "manifest_version": 3,
  "name": "Stacker 网页对话",
  "version": "0.1.0",
  "description": "整理、搜索、导出和清理 ChatGPT 与 Claude 网页对话。数据只存在本机。",
  "key": "<Step 2 输出>",
  "permissions": ["storage", "downloads"],
  "host_permissions": ["https://chatgpt.com/*", "https://claude.ai/*"],
  "background": { "service_worker": "background.js", "type": "module" },
  "action": { "default_title": "Stacker 网页对话" },
  "commands": {
    "open-popup": { "suggested_key": { "default": "Alt+Shift+S" }, "description": "打开对话小窗" }
  },
  "content_scripts": [
    { "matches": ["https://chatgpt.com/*", "https://claude.ai/*"], "js": ["content.js"], "run_at": "document_idle" }
  ]
}
```

- [ ] **Step 4: 写失败的测试**

`extension/src/manifest.test.ts`：

```ts
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
// @ts-expect-error plain ESM script without types
import { extensionId } from "../scripts/extension-id.mjs";

const manifest = JSON.parse(readFileSync(new URL("../public/manifest.json", import.meta.url), "utf8"));

describe("manifest", () => {
  it("asks only for the supported sites and never for all URLs", () => {
    expect(manifest.host_permissions).toEqual(["https://chatgpt.com/*", "https://claude.ai/*"]);
    expect(JSON.stringify(manifest)).not.toContain("<all_urls>");
    expect(manifest.permissions).toEqual(["storage", "downloads"]);
  });
  it("has a fixed ID recorded in EXTENSION_ID", () => {
    const recorded = readFileSync(new URL("../EXTENSION_ID", import.meta.url), "utf8").trim();
    expect(recorded).toMatch(/^[a-p]{32}$/);
    expect(extensionId(manifest.key)).toBe(recorded);
  });
});
```

`extension/src/i18n.test.ts`：

```ts
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { EN } from "./i18n";

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return name === "fixtures" ? [] : files(path);
    return /\.(ts|tsx)$/.test(name) && !name.endsWith(".test.ts") && !name.endsWith(".test.tsx") ? [path] : [];
  });
}

describe("i18n", () => {
  it("has an English entry for every t() literal", () => {
    const root = new URL(".", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
    const missing = files(root).flatMap((file) =>
      [...readFileSync(file, "utf8").matchAll(/\bt\("([^"]+)"\)/g)].map((m) => m[1]).filter((text) => !(text in EN)));
    expect(missing).toEqual([]);
  });
});
```

- [ ] **Step 5: 运行，确认失败**

Run: `npx vitest run extension/src`
Expected: FAIL（`./i18n` 不存在）。

- [ ] **Step 6: 实现 i18n、页面入口与构建配置**

`extension/src/i18n.ts`：

```ts
/** English for every Chinese UI string; Chinese is the source text. */
export const EN: Record<string, string> = {
  "Stacker 网页对话": "Stacker Web Chats",
};

const english = typeof navigator !== "undefined" && !navigator.language.toLowerCase().startsWith("zh");

export function t(text: string): string {
  return english ? EN[text] ?? text : text;
}
```

后续任务每新增一处 `t("…")` 都要在 `EN` 里补英文，`i18n.test.ts` 会检查。

`extension/manage.html`：

```html
<!doctype html>
<html lang="zh-CN">
  <head><meta charset="UTF-8" /><title>Stacker 网页对话</title></head>
  <body><div id="root"></div><script type="module" src="/src/ui/manage/main.tsx"></script></body>
</html>
```

`extension/popup.html` 同上，`src` 改为 `/src/ui/popup/main.tsx`，标题 `对话小窗`。

`extension/src/ui/manage/main.tsx`：

```tsx
import { createRoot } from "react-dom/client";
import { t } from "../../i18n";

createRoot(document.getElementById("root")!).render(<h1>{t("Stacker 网页对话")}</h1>);
```

`extension/src/ui/popup/main.tsx` 同上（Task 12 替换）。

`extension/src/background.ts`：

```ts
chrome.action.onClicked.addListener(() => {
  void chrome.tabs.create({ url: chrome.runtime.getURL("manage.html") });
});
```

`extension/src/content/index.ts`：

```ts
// Filled in by Task 5 (site RPC) and Task 13 (excerpt button).
export {};
```

`extension/vite.config.ts`：

```ts
import { resolve } from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const root = resolve(import.meta.dirname);

export default defineConfig({
  root,
  plugins: [react()],
  build: {
    outDir: resolve(root, "dist"),
    emptyOutDir: true,
    rollupOptions: {
      input: {
        manage: resolve(root, "manage.html"),
        popup: resolve(root, "popup.html"),
        background: resolve(root, "src/background.ts"),
      },
      output: { entryFileNames: "[name].js", chunkFileNames: "chunks/[name]-[hash].js", assetFileNames: "assets/[name]-[hash][extname]" },
    },
  },
});
```

`extension/vite.content.config.ts`（内容脚本不能是 ES 模块）：

```ts
import { resolve } from "node:path";
import { defineConfig } from "vite";

const root = resolve(import.meta.dirname);

export default defineConfig({
  root,
  publicDir: false,
  build: {
    outDir: resolve(root, "dist"),
    emptyOutDir: false,
    lib: { entry: resolve(root, "src/content/index.ts"), formats: ["iife"], name: "stackerContent", fileName: () => "content.js" },
  },
});
```

`extension/tsconfig.json`：

```json
{
  "compilerOptions": {
    "tsBuildInfoFile": "../node_modules/.tmp/tsconfig.extension.tsbuildinfo",
    "target": "es2023",
    "lib": ["ES2023", "DOM", "DOM.Iterable"],
    "module": "esnext",
    "types": ["vite/client", "chrome", "node"],
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "verbatimModuleSyntax": true,
    "moduleDetection": "force",
    "noEmit": true,
    "jsx": "react-jsx",
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "erasableSyntaxOnly": true,
    "noFallthroughCasesInSwitch": true
  },
  "include": ["src", "vite.config.ts", "vite.content.config.ts"]
}
```

根 `tsconfig.json` 的 `references` 追加 `{ "path": "./extension/tsconfig.json" }`。

根 `package.json` 的 `scripts` 追加：

```json
"ext:build": "vite build -c extension/vite.config.ts && vite build -c extension/vite.content.config.ts",
"ext:zip": "npm run ext:build && node extension/scripts/zip.mjs"
```

`eslint.config.js` 的 `globalIgnores` 追加 `'extension/dist'`、`'extension/dist/**'`。`.gitignore` 追加 `extension/dist/` 与 `extension/*.zip`。

- [ ] **Step 7: 运行测试与构建**

Run: `npx vitest run extension/src && npm run ext:build && npm run typecheck && npm run lint`
Expected: 测试 PASS；`extension/dist/` 下有 `manifest.json`、`manage.html`、`popup.html`、`background.js`、`content.js`。

- [ ] **Step 8: Commit**

```bash
git add extension package.json package-lock.json tsconfig.json eslint.config.js .gitignore
git commit -m "feat(extension): scaffold the web chat extension with a fixed ID"
```

---

### Task 2: 共享类型、错误码、HTTP 与结构校验

**Files:**
- Create: `extension/src/shared/types.ts`, `extension/src/sites/http.ts`, `extension/src/sites/guards.ts`, `extension/src/sites/guards.test.ts`, `extension/src/sites/http.test.ts`

**Interfaces:**
- Produces（`shared/types.ts`）：

```ts
export type SiteId = "chatgpt" | "claude";
export type Role = "user" | "assistant" | "system" | "tool";
export interface RemoteAccount { remoteId: string; label: string }
export interface RemoteConversation { id: string; title: string; createdAt: number; updatedAt: number; archived: boolean }
export interface Message { role: Role; text: string; at: number | null; attachments: string[] }
export interface RemoteBody { id: string; title: string; updatedAt: number; messages: Message[] }
export interface ListPage { items: RemoteConversation[]; next: string | null }
export type ErrorCode = "E_BROKEN" | "E_RATE" | "E_AUTH" | "E_NOT_FOUND" | "E_HTTP" | "E_NET" | "E_NO_TAB" | "E_NO_AGENT" | "E_ACCOUNT" | "E_CANCELLED";
export class SiteError extends Error {
  constructor(public code: ErrorCode, public detail = "", public retryAfterMs: number | null = null) { super(`${code}${detail ? `: ${detail}` : ""}`); }
}
export interface SerializedError { code: ErrorCode; detail: string; retryAfterMs: number | null }
export function serializeError(e: unknown): SerializedError
export function reviveError(e: SerializedError): SiteError
```

- `sites/http.ts`：

```ts
export interface FetchInit { method?: "GET" | "POST" | "PATCH" | "DELETE"; body?: unknown; headers?: Record<string, string> }
export interface FetchResult { status: number; json: unknown; retryAfter: string | null }
export type FetchJson = (url: string, init?: FetchInit) => Promise<FetchResult>;
export const browserFetchJson: FetchJson;
export function expectOk(res: FetchResult): unknown // throws SiteError
```

- `sites/guards.ts`：`obj(v, path)`, `arr(v, path)`, `str(v, path)`, `optStr(v)`, `bool(v)`, `time(v, path)`（ISO 字符串、秒或毫秒 → 毫秒），均在不符时抛 `SiteError("E_BROKEN", path)`。

- [ ] **Step 1: 写失败的测试**

`extension/src/sites/guards.test.ts`：

```ts
import { describe, expect, it } from "vitest";
import { SiteError } from "../shared/types";
import { arr, obj, str, time } from "./guards";

describe("guards", () => {
  it("reads times given as ISO text, seconds or milliseconds", () => {
    expect(time("2026-09-19T00:00:00Z", "t")).toBe(Date.UTC(2026, 8, 19));
    expect(time(1_789_776_000, "t")).toBe(1_789_776_000_000);
    expect(time(1_789_776_000_123, "t")).toBe(1_789_776_000_123);
  });
  it("rejects an unexpected shape as E_BROKEN naming the path", () => {
    expect(() => obj(null, "root")).toThrow(SiteError);
    try { str(5, "items[0].id"); } catch (e) { expect((e as SiteError).code).toBe("E_BROKEN"); expect((e as SiteError).detail).toBe("items[0].id"); }
    expect(() => arr({}, "items")).toThrow("E_BROKEN: items");
    expect(() => time("yesterday", "t")).toThrow("E_BROKEN: t");
  });
});
```

`extension/src/sites/http.test.ts`：

```ts
import { describe, expect, it } from "vitest";
import { reviveError, serializeError, SiteError } from "../shared/types";
import { expectOk } from "./http";

const res = (status: number, retryAfter: string | null = null) => ({ status, json: { ok: true }, retryAfter });

describe("expectOk", () => {
  it("maps statuses to error codes", () => {
    expect(expectOk(res(200))).toEqual({ ok: true });
    for (const [status, code] of [[401, "E_AUTH"], [403, "E_AUTH"], [404, "E_NOT_FOUND"], [500, "E_HTTP"]] as const) {
      expect(() => expectOk(res(status))).toThrow(code);
    }
  });
  it("carries Retry-After on 429 in milliseconds", () => {
    try { expectOk(res(429, "7")); } catch (e) { expect((e as SiteError).code).toBe("E_RATE"); expect((e as SiteError).retryAfterMs).toBe(7000); }
  });
  it("round-trips errors across messaging", () => {
    const e = reviveError(serializeError(new SiteError("E_RATE", "x", 5)));
    expect([e.code, e.detail, e.retryAfterMs]).toEqual(["E_RATE", "x", 5]);
    expect(serializeError(new Error("boom")).code).toBe("E_NET");
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/sites`
Expected: FAIL（模块不存在）。

- [ ] **Step 3: 实现**

`extension/src/shared/types.ts`：

```ts
export type SiteId = "chatgpt" | "claude";
export type Role = "user" | "assistant" | "system" | "tool";

export interface RemoteAccount { remoteId: string; label: string }
export interface RemoteConversation { id: string; title: string; createdAt: number; updatedAt: number; archived: boolean }
export interface Message { role: Role; text: string; at: number | null; attachments: string[] }
export interface RemoteBody { id: string; title: string; updatedAt: number; messages: Message[] }
export interface ListPage { items: RemoteConversation[]; next: string | null }

export type ErrorCode =
  | "E_BROKEN" | "E_RATE" | "E_AUTH" | "E_NOT_FOUND" | "E_HTTP" | "E_NET"
  | "E_NO_TAB" | "E_NO_AGENT" | "E_ACCOUNT" | "E_CANCELLED";

export class SiteError extends Error {
  constructor(public code: ErrorCode, public detail = "", public retryAfterMs: number | null = null) {
    super(`${code}${detail ? `: ${detail}` : ""}`);
  }
}

export interface SerializedError { code: ErrorCode; detail: string; retryAfterMs: number | null }

export function serializeError(e: unknown): SerializedError {
  if (e instanceof SiteError) return { code: e.code, detail: e.detail, retryAfterMs: e.retryAfterMs };
  return { code: "E_NET", detail: e instanceof Error ? e.message : String(e), retryAfterMs: null };
}

export function reviveError(e: SerializedError): SiteError {
  return new SiteError(e.code, e.detail, e.retryAfterMs);
}
```

`extension/src/sites/guards.ts`：

```ts
import { SiteError } from "../shared/types";

const broken = (path: string) => new SiteError("E_BROKEN", path);

export function obj(v: unknown, path: string): Record<string, unknown> {
  if (typeof v !== "object" || v === null || Array.isArray(v)) throw broken(path);
  return v as Record<string, unknown>;
}
export function arr(v: unknown, path: string): unknown[] {
  if (!Array.isArray(v)) throw broken(path);
  return v;
}
export function str(v: unknown, path: string): string {
  if (typeof v !== "string") throw broken(path);
  return v;
}
export function optStr(v: unknown): string {
  return typeof v === "string" ? v : "";
}
export function bool(v: unknown): boolean {
  return v === true;
}
/** ISO text, epoch seconds or epoch milliseconds → milliseconds. */
export function time(v: unknown, path: string): number {
  if (typeof v === "number" && Number.isFinite(v)) return v < 1e11 ? Math.round(v * 1000) : Math.round(v);
  if (typeof v === "string") {
    const ms = Date.parse(v);
    if (!Number.isNaN(ms)) return ms;
  }
  throw broken(path);
}
```

`extension/src/sites/http.ts`：

```ts
import { SiteError } from "../shared/types";

export interface FetchInit { method?: "GET" | "POST" | "PATCH" | "DELETE"; body?: unknown; headers?: Record<string, string> }
export interface FetchResult { status: number; json: unknown; retryAfter: string | null }
export type FetchJson = (url: string, init?: FetchInit) => Promise<FetchResult>;

/** Same-origin request from the site's page, so the page's sign-in cookies apply. */
export const browserFetchJson: FetchJson = async (url, init = {}) => {
  let res: Response;
  try {
    res = await fetch(url, {
      method: init.method ?? "GET",
      credentials: "include",
      headers: { ...(init.body !== undefined ? { "Content-Type": "application/json" } : {}), ...init.headers },
      body: init.body !== undefined ? JSON.stringify(init.body) : undefined,
    });
  } catch (e) {
    throw new SiteError("E_NET", e instanceof Error ? e.message : String(e));
  }
  const text = await res.text();
  let json: unknown = null;
  if (text) { try { json = JSON.parse(text); } catch { json = null; } }
  return { status: res.status, json, retryAfter: res.headers.get("Retry-After") };
};

export function expectOk(res: FetchResult): unknown {
  if (res.status >= 200 && res.status < 300) return res.json;
  if (res.status === 401 || res.status === 403) throw new SiteError("E_AUTH", String(res.status));
  if (res.status === 404) throw new SiteError("E_NOT_FOUND");
  if (res.status === 429) {
    const seconds = Number(res.retryAfter);
    throw new SiteError("E_RATE", "", Number.isFinite(seconds) && seconds > 0 ? seconds * 1000 : null);
  }
  throw new SiteError("E_HTTP", String(res.status));
}
```

- [ ] **Step 4: 运行测试**

Run: `npx vitest run extension/src/sites`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add extension/src/shared extension/src/sites
git commit -m "feat(extension): shared types, error codes and response guards"
```

---

### Task 3: ChatGPT 适配器

接口（执行前以 Task 15 的只读实测核对；形状不同时改样本与解析，不改对外接口）：

- `GET /api/auth/session` → `{ user: { id }, accessToken }`；未登录时无 `accessToken`。
- `GET /backend-api/conversations?offset=N&limit=100&order=updated[&is_archived=true]` → `{ items: [{ id, title, create_time, update_time, is_archived? }], total }`
- `GET /backend-api/conversation/{id}` → `{ title, update_time, current_node, mapping: { [node]: { id, parent, message: { author: { role }, create_time, content: { content_type, parts?, text? }, metadata? } | null } } }`
- `PATCH /backend-api/conversation/{id}` body `{ is_visible: false }` 删除；`{ is_archived: true }` 归档。

**Files:**
- Create: `extension/src/sites/types.ts`, `extension/src/sites/chatgpt.ts`, `extension/src/sites/chatgpt.test.ts`, `extension/src/sites/fixtures/chatgpt.ts`

**Interfaces:**
- Consumes: Task 2 全部。
- Produces（`sites/types.ts`）：

```ts
import type { ListPage, RemoteAccount, RemoteBody, SiteId } from "../shared/types";
import type { FetchJson } from "./http";
export interface Adapter {
  site: SiteId;
  origin: string;
  account(): Promise<RemoteAccount>;
  list(cursor: string | null): Promise<ListPage>;
  read(id: string): Promise<RemoteBody>;
  remove(id: string): Promise<void>;
  archive?: (id: string) => Promise<void>;
  conversationUrl(id: string): string;
}
export type AdapterFactory = (fetchJson: FetchJson) => Adapter;
```

- `chatgpt.ts`：`export const chatgpt: AdapterFactory`；`export function currentBranch(mapping, currentNode): Message[]`。

- [ ] **Step 1: 写样本**

`extension/src/sites/fixtures/chatgpt.ts`：

```ts
export const session = { user: { id: "user-abc", email: "someone@example.com" }, accessToken: "tok" };
export const listPage = {
  items: [
    { id: "c1", title: "Plan a trip", create_time: "2026-09-01T10:00:00Z", update_time: "2026-09-02T10:00:00Z", is_archived: false },
    { id: "c2", title: "Fix build", create_time: 1_788_000_000, update_time: 1_788_100_000.5 },
  ],
  total: 2, limit: 100, offset: 0,
};
export const conversation = {
  title: "Plan a trip",
  update_time: 1_788_200_000,
  current_node: "n4",
  mapping: {
    root: { id: "root", parent: null, message: null },
    n1: { id: "n1", parent: "root", message: { author: { role: "system" }, create_time: null, content: { content_type: "text", parts: [""] }, metadata: { is_visually_hidden_from_conversation: true } } },
    n2: { id: "n2", parent: "n1", message: { author: { role: "user" }, create_time: 1_788_000_000, content: { content_type: "text", parts: ["Where to go?"] }, metadata: { attachments: [{ name: "map.png" }] } } },
    n3old: { id: "n3old", parent: "n2", message: { author: { role: "assistant" }, create_time: 1_788_000_010, content: { content_type: "text", parts: ["Old branch"] } } },
    n3: { id: "n3", parent: "n2", message: { author: { role: "assistant" }, create_time: 1_788_000_020, content: { content_type: "text", parts: ["Try Kyoto."] } } },
    n4: { id: "n4", parent: "n3", message: { author: { role: "tool" }, create_time: 1_788_000_030, content: { content_type: "code", text: "print(1)" } } },
  },
};
```

- [ ] **Step 2: 写失败的测试**

`extension/src/sites/chatgpt.test.ts`：

```ts
import { describe, expect, it, vi } from "vitest";
import type { FetchInit, FetchResult } from "./http";
import { chatgpt } from "./chatgpt";
import { conversation, listPage, session } from "./fixtures/chatgpt";

function fake(routes: Record<string, unknown>, calls: [string, FetchInit | undefined][] = []) {
  return vi.fn(async (url: string, init?: FetchInit): Promise<FetchResult> => {
    calls.push([url, init]);
    const key = Object.keys(routes).find((k) => url.includes(k));
    return key ? { status: 200, json: routes[key], retryAfter: null } : { status: 404, json: null, retryAfter: null };
  });
}

describe("chatgpt adapter", () => {
  it("names the account by user id and never keeps the email", async () => {
    const a = chatgpt(fake({ "/api/auth/session": session }));
    const account = await a.account();
    expect(account).toEqual({ remoteId: "user-abc", label: "ChatGPT" });
    expect(JSON.stringify(account)).not.toContain("example.com");
  });

  it("lists live then archived pages with the bearer token", async () => {
    const calls: [string, FetchInit | undefined][] = [];
    const a = chatgpt(fake({ "/api/auth/session": session, "is_archived=true": { items: [], total: 0 }, "/backend-api/conversations": listPage }, calls));
    const first = await a.list(null);
    expect(first.items.map((i) => [i.id, i.archived])).toEqual([["c1", false], ["c2", false]]);
    expect(first.items[1].updatedAt).toBe(1_788_100_000_500);
    expect(first.next).toBe("archived:0");
    const second = await a.list(first.next);
    expect(second).toEqual({ items: [], next: null });
    const listCall = calls.find(([url]) => url.includes("/backend-api/conversations"))!;
    expect(listCall[1]?.headers?.Authorization).toBe("Bearer tok");
  });

  it("reads only the branch on screen, skipping hidden messages", async () => {
    const a = chatgpt(fake({ "/api/auth/session": session, "/backend-api/conversation/c1": conversation }));
    const body = await a.read("c1");
    expect(body.messages.map((m) => [m.role, m.text])).toEqual([["user", "Where to go?"], ["assistant", "Try Kyoto."], ["tool", "print(1)"]]);
    expect(body.messages[0].attachments).toEqual(["map.png"]);
  });

  it("deletes and archives with PATCH", async () => {
    const calls: [string, FetchInit | undefined][] = [];
    const a = chatgpt(fake({ "/api/auth/session": session, "/backend-api/conversation/c1": { success: true } }, calls));
    await a.remove("c1");
    await a.archive!("c1");
    const patches = calls.filter(([, init]) => init?.method === "PATCH").map(([, init]) => init?.body);
    expect(patches).toEqual([{ is_visible: false }, { is_archived: true }]);
  });

  it("treats a missing token as signed out and a changed shape as broken", async () => {
    await expect(chatgpt(fake({ "/api/auth/session": {} })).account()).rejects.toThrow("E_AUTH");
    const a = chatgpt(fake({ "/api/auth/session": session, "/backend-api/conversations": { conversations: [] } }));
    await expect(a.list(null)).rejects.toThrow("E_BROKEN");
  });
});
```

- [ ] **Step 3: 运行，确认失败**

Run: `npx vitest run extension/src/sites/chatgpt.test.ts`
Expected: FAIL（模块不存在）。

- [ ] **Step 4: 实现**

`extension/src/sites/types.ts`：按上方 Interfaces 原文写入。

`extension/src/sites/chatgpt.ts`：

```ts
import { SiteError, type Message, type RemoteConversation, type Role } from "../shared/types";
import { arr, bool, obj, optStr, str, time } from "./guards";
import { expectOk, type FetchInit } from "./http";
import type { AdapterFactory } from "./types";

const ORIGIN = "https://chatgpt.com";
const PAGE = 100;
const ROLES: Role[] = ["user", "assistant", "system", "tool"];

/** Messages from the root to the node on screen; other branches are left out. */
export function currentBranch(mappingValue: unknown, currentNode: string): Message[] {
  const mapping = obj(mappingValue, "mapping");
  const chain: Record<string, unknown>[] = [];
  const seen = new Set<string>();
  let id: string | null = currentNode;
  while (id && !seen.has(id)) {
    seen.add(id);
    const node = obj(mapping[id], `mapping.${id}`);
    chain.push(node);
    id = typeof node.parent === "string" ? node.parent : null;
  }
  return chain.reverse().flatMap((node): Message[] => {
    if (node.message == null) return [];
    const message = obj(node.message, "message");
    const metadata = typeof message.metadata === "object" && message.metadata ? (message.metadata as Record<string, unknown>) : {};
    if (metadata.is_visually_hidden_from_conversation === true) return [];
    const role = str(obj(message.author, "message.author").role, "message.author.role") as Role;
    if (!ROLES.includes(role)) throw new SiteError("E_BROKEN", "message.author.role");
    const content = obj(message.content, "message.content");
    const parts = Array.isArray(content.parts) ? content.parts : [];
    const text = [...parts.filter((p): p is string => typeof p === "string"), optStr(content.text)].filter(Boolean).join("\n").trim();
    const attachments = Array.isArray(metadata.attachments)
      ? metadata.attachments.map((a) => optStr((a as Record<string, unknown>)?.name)).filter(Boolean)
      : [];
    if (!text && !attachments.length) return [];
    return [{ role, text, at: message.create_time == null ? null : time(message.create_time, "message.create_time"), attachments }];
  });
}

export const chatgpt: AdapterFactory = (fetchJson) => {
  let token: string | null = null;

  async function auth(): Promise<string> {
    if (token) return token;
    const session = obj(expectOk(await fetchJson(`${ORIGIN}/api/auth/session`)), "session");
    if (typeof session.accessToken !== "string" || !session.accessToken) throw new SiteError("E_AUTH", "no session");
    token = session.accessToken;
    return token;
  }
  async function api(path: string, init: FetchInit = {}): Promise<unknown> {
    const bearer = await auth();
    return expectOk(await fetchJson(`${ORIGIN}${path}`, { ...init, headers: { ...init.headers, Authorization: `Bearer ${bearer}` } }));
  }

  return {
    site: "chatgpt",
    origin: ORIGIN,
    conversationUrl: (id) => `${ORIGIN}/c/${id}`,

    async account() {
      const session = obj(expectOk(await fetchJson(`${ORIGIN}/api/auth/session`)), "session");
      if (typeof session.accessToken !== "string" || !session.accessToken) throw new SiteError("E_AUTH", "no session");
      token = session.accessToken;
      return { remoteId: str(obj(session.user, "session.user").id, "session.user.id"), label: "ChatGPT" };
    },

    /** Cursor is `live:<offset>` or `archived:<offset>`; live pages come first. */
    async list(cursor) {
      const [phase, offsetText] = (cursor ?? "live:0").split(":");
      const offset = Number(offsetText) || 0;
      const archived = phase === "archived";
      const page = obj(await api(`/backend-api/conversations?offset=${offset}&limit=${PAGE}&order=updated${archived ? "&is_archived=true" : ""}`), "page");
      const items: RemoteConversation[] = arr(page.items, "page.items").map((raw, i) => {
        const item = obj(raw, `items[${i}]`);
        return {
          id: str(item.id, `items[${i}].id`),
          title: optStr(item.title),
          createdAt: time(item.create_time, `items[${i}].create_time`),
          updatedAt: time(item.update_time, `items[${i}].update_time`),
          archived: archived || bool(item.is_archived),
        };
      });
      const total = typeof page.total === "number" ? page.total : offset + items.length;
      const more = items.length > 0 && offset + items.length < total;
      const next = more ? `${phase}:${offset + items.length}` : archived ? null : "archived:0";
      return { items, next };
    },

    async read(id) {
      const conv = obj(await api(`/backend-api/conversation/${encodeURIComponent(id)}`), "conversation");
      return {
        id,
        title: optStr(conv.title),
        updatedAt: time(conv.update_time, "conversation.update_time"),
        messages: currentBranch(conv.mapping, str(conv.current_node, "conversation.current_node")),
      };
    },

    async remove(id) {
      await api(`/backend-api/conversation/${encodeURIComponent(id)}`, { method: "PATCH", body: { is_visible: false } });
    },

    async archive(id) {
      await api(`/backend-api/conversation/${encodeURIComponent(id)}`, { method: "PATCH", body: { is_archived: true } });
    },
  };
};
```

- [ ] **Step 5: 运行测试**

Run: `npx vitest run extension/src/sites/chatgpt.test.ts`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add extension/src/sites
git commit -m "feat(extension): ChatGPT adapter with branch-aware reading"
```

---

### Task 4: Claude 适配器与站点表

接口（执行前以 Task 15 核对）：

- `GET /api/organizations` → `[{ uuid, name, capabilities?: string[] }]`；取第一个含 `"chat"` 能力的组织，没有则第一个。
- `GET /api/organizations/{org}/chat_conversations?limit=100&offset=N` → `[{ uuid, name, created_at, updated_at, is_archived? }]`
- `GET /api/organizations/{org}/chat_conversations/{id}?tree=True&rendering_mode=messages&render_all_tools=true` → `{ uuid, name, updated_at, current_leaf_message_uuid?, chat_messages: [{ uuid, parent_message_uuid?, sender: "human"|"assistant", text?, content?: [{ type, text? }], created_at, attachments?: [{ file_name }], files?: [{ file_name }] }] }`
- `DELETE /api/organizations/{org}/chat_conversations/{id}` → 200 或 204。
- Claude 没有归档。

**Files:**
- Create: `extension/src/sites/claude.ts`, `extension/src/sites/claude.test.ts`, `extension/src/sites/fixtures/claude.ts`, `extension/src/sites/registry.ts`, `extension/src/sites/registry.test.ts`

**Interfaces:**
- Consumes: Task 2、Task 3 的 `Adapter`、`AdapterFactory`。
- Produces: `export const claude: AdapterFactory`；`registry.ts`：

```ts
export const SITES: Record<SiteId, { label: string; factory: AdapterFactory; origin: string; match: string }>;
export function siteOfUrl(url: string): SiteId | null;
export function conversationIdOfUrl(site: SiteId, url: string): string | null;
export function conversationUrl(site: SiteId, id: string): string;
```

- [ ] **Step 1: 写样本**

`extension/src/sites/fixtures/claude.ts`：

```ts
export const orgs = [
  { uuid: "org-api", name: "API", capabilities: ["api"] },
  { uuid: "org-chat", name: "Personal", capabilities: ["chat", "claude_pro"] },
];
export const listFull = Array.from({ length: 100 }, (_, i) => ({ uuid: `k${i}`, name: `Chat ${i}`, created_at: "2026-09-01T00:00:00Z", updated_at: "2026-09-02T00:00:00Z" }));
export const listTail = [{ uuid: "k100", name: "", created_at: "2026-08-01T00:00:00Z", updated_at: "2026-08-02T00:00:00Z" }];
export const conversation = {
  uuid: "k1",
  name: "Refactor",
  updated_at: "2026-09-02T00:00:00Z",
  current_leaf_message_uuid: "m3",
  chat_messages: [
    { uuid: "m1", parent_message_uuid: "00000000-0000-4000-8000-000000000000", sender: "human", text: "Help me refactor", content: [{ type: "text", text: "Help me refactor" }], created_at: "2026-09-02T00:00:00Z", attachments: [{ file_name: "a.ts" }], files: [] },
    { uuid: "m2old", parent_message_uuid: "m1", sender: "assistant", content: [{ type: "text", text: "Old answer" }], created_at: "2026-09-02T00:00:01Z" },
    { uuid: "m3", parent_message_uuid: "m1", sender: "assistant", content: [{ type: "tool_use", name: "x" }, { type: "text", text: "Here is the plan." }], created_at: "2026-09-02T00:00:02Z" },
  ],
};
```

- [ ] **Step 2: 写失败的测试**

`extension/src/sites/claude.test.ts`：

```ts
import { describe, expect, it, vi } from "vitest";
import type { FetchInit, FetchResult } from "./http";
import { claude } from "./claude";
import { conversation, listFull, listTail, orgs } from "./fixtures/claude";

function fake(route: (url: string, init?: FetchInit) => unknown, calls: [string, FetchInit | undefined][] = []) {
  return vi.fn(async (url: string, init?: FetchInit): Promise<FetchResult> => {
    calls.push([url, init]);
    const json = route(url, init);
    return json === undefined ? { status: 404, json: null, retryAfter: null } : { status: init?.method === "DELETE" ? 204 : 200, json, retryAfter: null };
  });
}
const routes = (url: string, init?: FetchInit) => {
  if (init?.method === "DELETE") return null;
  if (url.endsWith("/api/organizations")) return orgs;
  if (url.includes("chat_conversations/k1")) return conversation;
  if (url.includes("offset=0")) return listFull;
  if (url.includes("offset=100")) return listTail;
  return undefined;
};

describe("claude adapter", () => {
  it("uses the chat organization as the account", async () => {
    expect(await claude(fake(routes)).account()).toEqual({ remoteId: "org-chat", label: "Claude" });
  });
  it("pages by offset until a short page", async () => {
    const a = claude(fake(routes));
    const first = await a.list(null);
    expect(first.items).toHaveLength(100);
    expect(first.next).toBe("100");
    const second = await a.list(first.next);
    expect(second.items.map((i) => i.id)).toEqual(["k100"]);
    expect(second.next).toBeNull();
  });
  it("reads the branch ending at the current leaf, text blocks only", async () => {
    const body = await claude(fake(routes)).read("k1");
    expect(body.messages.map((m) => [m.role, m.text])).toEqual([["user", "Help me refactor"], ["assistant", "Here is the plan."]]);
    expect(body.messages[0].attachments).toEqual(["a.ts"]);
  });
  it("deletes with DELETE and has no archive", async () => {
    const calls: [string, FetchInit | undefined][] = [];
    const a = claude(fake(routes, calls));
    await a.remove("k1");
    expect(calls.some(([url, init]) => init?.method === "DELETE" && url.endsWith("/chat_conversations/k1"))).toBe(true);
    expect(a.archive).toBeUndefined();
  });
  it("flags a changed shape as broken", async () => {
    const a = claude(fake((url) => (url.endsWith("/api/organizations") ? orgs : { conversations: [] })));
    await expect(a.list(null)).rejects.toThrow("E_BROKEN");
  });
});
```

`extension/src/sites/registry.test.ts`：

```ts
import { describe, expect, it } from "vitest";
import { conversationIdOfUrl, conversationUrl, siteOfUrl } from "./registry";

describe("registry", () => {
  it("recognises sites and conversation ids from URLs", () => {
    expect(siteOfUrl("https://chatgpt.com/c/abc-1")).toBe("chatgpt");
    expect(siteOfUrl("https://claude.ai/chat/k1")).toBe("claude");
    expect(siteOfUrl("https://example.com/")).toBeNull();
    expect(conversationIdOfUrl("chatgpt", "https://chatgpt.com/g/g-x/c/abc-1?model=x")).toBe("abc-1");
    expect(conversationIdOfUrl("claude", "https://claude.ai/chat/k1")).toBe("k1");
    expect(conversationIdOfUrl("claude", "https://claude.ai/new")).toBeNull();
    expect(conversationUrl("chatgpt", "abc")).toBe("https://chatgpt.com/c/abc");
  });
});
```

- [ ] **Step 3: 运行，确认失败**

Run: `npx vitest run extension/src/sites`
Expected: FAIL（`./claude`、`./registry` 不存在）。

- [ ] **Step 4: 实现**

`extension/src/sites/claude.ts`：

```ts
import { SiteError, type Message } from "../shared/types";
import { arr, obj, optStr, str, time } from "./guards";
import { expectOk, type FetchInit } from "./http";
import type { AdapterFactory } from "./types";

const ORIGIN = "https://claude.ai";
const PAGE = 100;

function messageText(m: Record<string, unknown>): string {
  if (Array.isArray(m.content)) {
    const parts = m.content
      .map((b) => (typeof b === "object" && b ? (b as Record<string, unknown>) : {}))
      .filter((b) => b.type === "text")
      .map((b) => optStr(b.text));
    if (parts.length) return parts.join("\n").trim();
  }
  return optStr(m.text).trim();
}

function fileNames(v: unknown): string[] {
  return Array.isArray(v) ? v.map((f) => optStr((f as Record<string, unknown>)?.file_name)).filter(Boolean) : [];
}

/** The chain from the current leaf back to the root; with no leaf, the messages in order. */
function branch(messages: Record<string, unknown>[], leaf: string | null): Record<string, unknown>[] {
  if (!leaf) return messages;
  const byId = new Map(messages.map((m) => [optStr(m.uuid), m]));
  const chain: Record<string, unknown>[] = [];
  const seen = new Set<string>();
  let id: string | null = leaf;
  while (id && byId.has(id) && !seen.has(id)) {
    seen.add(id);
    const m: Record<string, unknown> = byId.get(id)!;
    chain.push(m);
    id = optStr(m.parent_message_uuid) || null;
  }
  return chain.reverse();
}

export const claude: AdapterFactory = (fetchJson) => {
  let org: string | null = null;

  async function get(path: string, init: FetchInit = {}): Promise<unknown> {
    return expectOk(await fetchJson(`${ORIGIN}${path}`, init));
  }
  async function orgId(): Promise<string> {
    if (org) return org;
    const list = arr(await get("/api/organizations"), "organizations").map((o, i) => obj(o, `organizations[${i}]`));
    if (!list.length) throw new SiteError("E_AUTH", "no organization");
    const chat = list.find((o) => Array.isArray(o.capabilities) && o.capabilities.includes("chat")) ?? list[0];
    org = str(chat.uuid, "organizations[].uuid");
    return org;
  }

  return {
    site: "claude",
    origin: ORIGIN,
    conversationUrl: (id) => `${ORIGIN}/chat/${id}`,

    async account() {
      return { remoteId: await orgId(), label: "Claude" };
    },

    async list(cursor) {
      const offset = Number(cursor ?? 0) || 0;
      const rows = arr(await get(`/api/organizations/${await orgId()}/chat_conversations?limit=${PAGE}&offset=${offset}`), "chat_conversations");
      const items = rows.map((raw, i) => {
        const c = obj(raw, `chat_conversations[${i}]`);
        return {
          id: str(c.uuid, `chat_conversations[${i}].uuid`),
          title: optStr(c.name),
          createdAt: time(c.created_at, `chat_conversations[${i}].created_at`),
          updatedAt: time(c.updated_at, `chat_conversations[${i}].updated_at`),
          archived: c.is_archived === true,
        };
      });
      // A server that ignores `limit` returns everything at once.
      return { items, next: rows.length === PAGE ? String(offset + PAGE) : null };
    },

    async read(id) {
      const c = obj(await get(`/api/organizations/${await orgId()}/chat_conversations/${encodeURIComponent(id)}?tree=True&rendering_mode=messages&render_all_tools=true`), "conversation");
      const all = arr(c.chat_messages, "conversation.chat_messages").map((m, i) => obj(m, `chat_messages[${i}]`));
      const messages: Message[] = branch(all, optStr(c.current_leaf_message_uuid) || null).flatMap((m, i): Message[] => {
        const sender = str(m.sender, `chat_messages[${i}].sender`);
        if (sender !== "human" && sender !== "assistant") throw new SiteError("E_BROKEN", `chat_messages[${i}].sender`);
        const text = messageText(m);
        const attachments = [...fileNames(m.attachments), ...fileNames(m.files)];
        if (!text && !attachments.length) return [];
        return [{ role: sender === "human" ? "user" : "assistant", text, at: m.created_at == null ? null : time(m.created_at, `chat_messages[${i}].created_at`), attachments }];
      });
      return { id, title: optStr(c.name), updatedAt: time(c.updated_at, "conversation.updated_at"), messages };
    },

    async remove(id) {
      await get(`/api/organizations/${await orgId()}/chat_conversations/${encodeURIComponent(id)}`, { method: "DELETE" });
    },
  };
};
```

`extension/src/sites/registry.ts`：

```ts
import type { SiteId } from "../shared/types";
import { chatgpt } from "./chatgpt";
import { claude } from "./claude";
import type { AdapterFactory } from "./types";

export const SITES: Record<SiteId, { label: string; factory: AdapterFactory; origin: string; match: string }> = {
  chatgpt: { label: "ChatGPT", factory: chatgpt, origin: "https://chatgpt.com", match: "https://chatgpt.com/*" },
  claude: { label: "Claude", factory: claude, origin: "https://claude.ai", match: "https://claude.ai/*" },
};

export function siteOfUrl(url: string): SiteId | null {
  const origin = (() => { try { return new URL(url).origin; } catch { return ""; } })();
  return (Object.keys(SITES) as SiteId[]).find((id) => SITES[id].origin === origin) ?? null;
}

const ID_PATTERN: Record<SiteId, RegExp> = {
  chatgpt: /\/c\/([A-Za-z0-9-]+)/,
  claude: /\/chat\/([A-Za-z0-9-]+)/,
};

export function conversationIdOfUrl(site: SiteId, url: string): string | null {
  const path = (() => { try { return new URL(url).pathname; } catch { return ""; } })();
  return ID_PATTERN[site].exec(path)?.[1] ?? null;
}

const URL_OF: Record<SiteId, (id: string) => string> = {
  chatgpt: (id) => `https://chatgpt.com/c/${id}`,
  claude: (id) => `https://claude.ai/chat/${id}`,
};

export function conversationUrl(site: SiteId, id: string): string {
  return URL_OF[site](id);
}
```

- [ ] **Step 5: 运行测试**

Run: `npx vitest run extension/src/sites`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add extension/src/sites
git commit -m "feat(extension): Claude adapter and site registry"
```

---

### Task 5: 内容脚本 RPC 与管理页的站点调用

**Files:**
- Create: `extension/src/content/agent.ts`, `extension/src/content/agent.test.ts`, `extension/src/lib/siteClient.ts`, `extension/src/lib/siteClient.test.ts`
- Modify: `extension/src/content/index.ts`

**Interfaces:**
- Consumes: `SITES`、`siteOfUrl`（Task 4），`browserFetchJson`（Task 2），`serializeError`/`reviveError`/`SiteError`。
- Produces:

```ts
// content/agent.ts
export type SiteOp = "account" | "list" | "read" | "remove" | "archive";
export interface SiteRequest { type: "site-rpc"; site: SiteId; op: SiteOp; arg: string | null }
export type SiteResponse = { ok: true; value: unknown } | { ok: false; error: SerializedError };
export function createAgent(adapter: Adapter): (req: SiteRequest) => Promise<SiteResponse>;
export function installAgent(): void; // registers chrome.runtime.onMessage in the page

// lib/siteClient.ts
export interface SiteApi {
  account(site: SiteId): Promise<RemoteAccount>;
  list(site: SiteId, cursor: string | null): Promise<ListPage>;
  read(site: SiteId, id: string): Promise<RemoteBody>;
  remove(site: SiteId, id: string): Promise<void>;
  archive(site: SiteId, id: string): Promise<void>;
  canArchive(site: SiteId): boolean;
}
export function createSiteApi(chromeApi?: Pick<typeof chrome, "tabs">): SiteApi;
```

- [ ] **Step 1: 写失败的测试**

`extension/src/content/agent.test.ts`：

```ts
import { describe, expect, it } from "vitest";
import { SiteError } from "../shared/types";
import type { Adapter } from "../sites/types";
import { createAgent } from "./agent";

const adapter: Adapter = {
  site: "claude", origin: "https://claude.ai", conversationUrl: (id) => id,
  account: async () => ({ remoteId: "org", label: "Claude" }),
  list: async () => { throw new SiteError("E_RATE", "", 3000); },
  read: async (id) => ({ id, title: "", updatedAt: 1, messages: [] }),
  remove: async () => {},
};

describe("content agent", () => {
  it("answers ops for its own site and serializes errors", async () => {
    const agent = createAgent(adapter);
    expect(await agent({ type: "site-rpc", site: "claude", op: "account", arg: null })).toEqual({ ok: true, value: { remoteId: "org", label: "Claude" } });
    expect(await agent({ type: "site-rpc", site: "claude", op: "list", arg: null })).toEqual({ ok: false, error: { code: "E_RATE", detail: "", retryAfterMs: 3000 } });
    expect(await agent({ type: "site-rpc", site: "claude", op: "archive", arg: "x" })).toMatchObject({ ok: false, error: { code: "E_HTTP" } });
    expect(await agent({ type: "site-rpc", site: "chatgpt", op: "account", arg: null })).toMatchObject({ ok: false, error: { code: "E_NO_AGENT" } });
  });
});
```

`extension/src/lib/siteClient.test.ts`：

```ts
import { describe, expect, it, vi } from "vitest";
import { createSiteApi } from "./siteClient";

function tabs(found: { id: number }[], reply: unknown) {
  return {
    query: vi.fn(async () => found),
    sendMessage: vi.fn(async () => reply),
  } as unknown as typeof chrome.tabs;
}

describe("site client", () => {
  it("sends the op to a tab of that site and returns the value", async () => {
    const t = tabs([{ id: 7 }], { ok: true, value: { remoteId: "u", label: "ChatGPT" } });
    const api = createSiteApi({ tabs: t });
    expect(await api.account("chatgpt")).toEqual({ remoteId: "u", label: "ChatGPT" });
    expect(t.query).toHaveBeenCalledWith({ url: "https://chatgpt.com/*" });
    expect(t.sendMessage).toHaveBeenCalledWith(7, { type: "site-rpc", site: "chatgpt", op: "account", arg: null });
  });
  it("reports a missing tab, a tab without the agent, and revives site errors", async () => {
    await expect(createSiteApi({ tabs: tabs([], null) }).account("claude")).rejects.toThrow("E_NO_TAB");
    const noAgent = { query: vi.fn(async () => [{ id: 1 }]), sendMessage: vi.fn(async () => { throw new Error("Receiving end does not exist."); }) } as unknown as typeof chrome.tabs;
    await expect(createSiteApi({ tabs: noAgent }).account("claude")).rejects.toThrow("E_NO_AGENT");
    const failing = tabs([{ id: 1 }], { ok: false, error: { code: "E_AUTH", detail: "401", retryAfterMs: null } });
    await expect(createSiteApi({ tabs: failing }).read("claude", "k")).rejects.toThrow("E_AUTH: 401");
  });
  it("knows which sites can archive", () => {
    const api = createSiteApi({ tabs: tabs([], null) });
    expect(api.canArchive("chatgpt")).toBe(true);
    expect(api.canArchive("claude")).toBe(false);
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/content extension/src/lib`
Expected: FAIL（模块不存在）。

- [ ] **Step 3: 实现**

`extension/src/content/agent.ts`：

```ts
import { serializeError, SiteError, type SerializedError, type SiteId } from "../shared/types";
import { browserFetchJson } from "../sites/http";
import { siteOfUrl, SITES } from "../sites/registry";
import type { Adapter } from "../sites/types";

export type SiteOp = "account" | "list" | "read" | "remove" | "archive";
export interface SiteRequest { type: "site-rpc"; site: SiteId; op: SiteOp; arg: string | null }
export type SiteResponse = { ok: true; value: unknown } | { ok: false; error: SerializedError };

export function createAgent(adapter: Adapter) {
  return async (req: SiteRequest): Promise<SiteResponse> => {
    try {
      if (req.site !== adapter.site) throw new SiteError("E_NO_AGENT", req.site);
      const arg = req.arg ?? "";
      let value: unknown;
      switch (req.op) {
        case "account": value = await adapter.account(); break;
        case "list": value = await adapter.list(req.arg); break;
        case "read": value = await adapter.read(arg); break;
        case "remove": await adapter.remove(arg); value = null; break;
        case "archive":
          if (!adapter.archive) throw new SiteError("E_HTTP", "archive unsupported");
          await adapter.archive(arg); value = null; break;
      }
      return { ok: true, value };
    } catch (e) {
      return { ok: false, error: serializeError(e) };
    }
  };
}

/** Runs in the site's page: requests are same-origin and carry the page's sign-in. */
export function installAgent(): void {
  const site = siteOfUrl(location.href);
  if (!site) return;
  const agent = createAgent(SITES[site].factory(browserFetchJson));
  chrome.runtime.onMessage.addListener((message: unknown, _sender, reply) => {
    const req = message as SiteRequest;
    if (req?.type !== "site-rpc") return false;
    void agent(req).then(reply);
    return true;
  });
}
```

`extension/src/content/index.ts`：

```ts
import { installAgent } from "./agent";

installAgent();
```

`extension/src/lib/siteClient.ts`：

```ts
import { reviveError, SiteError, type ListPage, type RemoteAccount, type RemoteBody, type SiteId } from "../shared/types";
import type { SiteOp, SiteRequest, SiteResponse } from "../content/agent";
import { SITES } from "../sites/registry";

export interface SiteApi {
  account(site: SiteId): Promise<RemoteAccount>;
  list(site: SiteId, cursor: string | null): Promise<ListPage>;
  read(site: SiteId, id: string): Promise<RemoteBody>;
  remove(site: SiteId, id: string): Promise<void>;
  archive(site: SiteId, id: string): Promise<void>;
  canArchive(site: SiteId): boolean;
}

export function createSiteApi(chromeApi: Pick<typeof chrome, "tabs"> = chrome): SiteApi {
  async function call<T>(site: SiteId, op: SiteOp, arg: string | null): Promise<T> {
    const tabs = await chromeApi.tabs.query({ url: SITES[site].match });
    const tab = tabs.find((t) => typeof t.id === "number");
    if (!tab?.id) throw new SiteError("E_NO_TAB", site);
    const req: SiteRequest = { type: "site-rpc", site, op, arg };
    let res: SiteResponse | undefined;
    try {
      res = (await chromeApi.tabs.sendMessage(tab.id, req)) as SiteResponse | undefined;
    } catch {
      throw new SiteError("E_NO_AGENT", site);
    }
    if (!res) throw new SiteError("E_NO_AGENT", site);
    if (!res.ok) throw reviveError(res.error);
    return res.value as T;
  }
  return {
    account: (site) => call(site, "account", null),
    list: (site, cursor) => call(site, "list", cursor),
    read: (site, id) => call(site, "read", id),
    remove: async (site, id) => { await call(site, "remove", id); },
    archive: async (site, id) => { await call(site, "archive", id); },
    canArchive: (site) => site === "chatgpt",
  };
}
```

- [ ] **Step 4: 运行测试**

Run: `npx vitest run extension/src/content extension/src/lib`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add extension/src/content extension/src/lib
git commit -m "feat(extension): site RPC between the manage page and site tabs"
```

---

### Task 6: 本地数据库

**Files:**
- Create: `extension/src/lib/db.ts`, `extension/src/lib/db.test.ts`

**Interfaces:**
- Consumes: `shared/types.ts`。
- Produces:

```ts
export interface Account { key: string; site: SiteId; remoteId: string; alias: string; lastSeen: number }
export interface LocalFields { folderId: string | null; tags: string[]; favorite: boolean; note: string }
export interface Conversation extends LocalFields {
  key: string; site: SiteId; account: string; id: string; title: string;
  createdAt: number; updatedAt: number; archived: boolean;
  bodyFetchedAt: number | null; bodyUpdatedAt: number | null; removedAt: number | null;
}
export interface StoredBody extends RemoteBody { key: string }
export interface Folder { id: string; name: string; createdAt: number }
export interface Excerpt { id: string; site: SiteId; conversationId: string | null; url: string; pageTitle: string; text: string; note: string; createdAt: number }
export type Db = IDBPDatabase<Schema>;
export function openDb(name?: string): Promise<Db>;
export const conversationKey: (site: SiteId, id: string) => string;
export const accountKey: (site: SiteId, remoteId: string) => string;
export function upsertAccount(db: Db, site: SiteId, remote: RemoteAccount, now: number): Promise<Account>;
export function listAccounts(db: Db): Promise<Account[]>;
export function renameAccount(db: Db, key: string, alias: string): Promise<void>;
export function mergeListing(db: Db, account: Account, items: RemoteConversation[], complete: boolean, now: number): Promise<{ added: number; updated: number; removed: number }>;
export function listConversations(db: Db): Promise<Conversation[]>;
export function getConversation(db: Db, key: string): Promise<Conversation | undefined>;
export function updateLocal(db: Db, keys: string[], patch: Partial<LocalFields>): Promise<void>;
export function addTag(db: Db, keys: string[], tag: string): Promise<void>;
export function putBody(db: Db, key: string, body: RemoteBody, now: number): Promise<void>;
export function getBody(db: Db, key: string): Promise<StoredBody | undefined>;
export function bodyIsFresh(c: Conversation): boolean;
export function markRemoved(db: Db, key: string, now: number): Promise<void>;
export function listFolders(db: Db): Promise<Folder[]>;
export function createFolder(db: Db, name: string, now: number): Promise<Folder>;
export function renameFolder(db: Db, id: string, name: string): Promise<void>;
export function deleteFolder(db: Db, id: string): Promise<void>;
export function addExcerpt(db: Db, e: Omit<Excerpt, "id" | "createdAt">, now: number): Promise<Excerpt>;
export function listExcerpts(db: Db, site?: SiteId, conversationId?: string): Promise<Excerpt[]>;
export function deleteExcerpt(db: Db, id: string): Promise<void>;
```

- [ ] **Step 1: 写失败的测试**

`extension/src/lib/db.test.ts`：

```ts
import "fake-indexeddb/auto";
import { beforeEach, describe, expect, it } from "vitest";
import {
  addExcerpt, addTag, bodyIsFresh, createFolder, deleteFolder, getBody, getConversation, listConversations,
  listExcerpts, markRemoved, mergeListing, openDb, putBody, updateLocal, upsertAccount, type Db,
} from "./db";

let db: Db;
let n = 0;
beforeEach(async () => { db = await openDb(`test-${n++}`); });

const item = (id: string, updatedAt = 2) => ({ id, title: id.toUpperCase(), createdAt: 1, updatedAt, archived: false });

describe("db", () => {
  it("keeps local fields when a listing refreshes a conversation", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u1", label: "ChatGPT" }, 10);
    expect(account).toMatchObject({ key: "chatgpt:u1", alias: "ChatGPT" });
    expect(await mergeListing(db, account, [item("a"), item("b")], true, 10)).toEqual({ added: 2, updated: 0, removed: 0 });
    await updateLocal(db, ["chatgpt:a"], { favorite: true, note: "keep" });
    await addTag(db, ["chatgpt:a", "chatgpt:b"], "work");
    expect(await mergeListing(db, account, [item("a", 5)], true, 20)).toEqual({ added: 0, updated: 1, removed: 1 });
    const a = await getConversation(db, "chatgpt:a");
    expect(a).toMatchObject({ updatedAt: 5, favorite: true, note: "keep", tags: ["work"], removedAt: null });
    expect((await getConversation(db, "chatgpt:b"))?.removedAt).toBe(20);
  });

  it("only marks missing ones removed after a complete listing of that account", async () => {
    const one = await upsertAccount(db, "claude", { remoteId: "o1", label: "Claude" }, 1);
    const two = await upsertAccount(db, "claude", { remoteId: "o2", label: "Claude" }, 1);
    expect(two.alias).toBe("Claude 2");
    await mergeListing(db, one, [item("x")], true, 1);
    await mergeListing(db, two, [item("y")], true, 1);
    await mergeListing(db, one, [], false, 2);
    expect((await getConversation(db, "claude:x"))?.removedAt).toBeNull();
    await mergeListing(db, one, [], true, 3);
    expect((await getConversation(db, "claude:x"))?.removedAt).toBe(3);
    expect((await getConversation(db, "claude:y"))?.removedAt).toBeNull();
  });

  it("stores bodies and tracks whether they are fresh", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "ChatGPT" }, 1);
    await mergeListing(db, account, [item("a", 5)], true, 1);
    await putBody(db, "chatgpt:a", { id: "a", title: "A", updatedAt: 5, messages: [] }, 9);
    const c = (await getConversation(db, "chatgpt:a"))!;
    expect([c.bodyFetchedAt, c.bodyUpdatedAt, bodyIsFresh(c)]).toEqual([9, 5, true]);
    expect((await getBody(db, "chatgpt:a"))?.title).toBe("A");
    await mergeListing(db, account, [item("a", 6)], true, 10);
    expect(bodyIsFresh((await getConversation(db, "chatgpt:a"))!)).toBe(false);
    await markRemoved(db, "chatgpt:a", 11);
    expect((await listConversations(db))[0].removedAt).toBe(11);
  });

  it("clears a deleted folder from its conversations and stores excerpts", async () => {
    const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "ChatGPT" }, 1);
    await mergeListing(db, account, [item("a")], true, 1);
    const folder = await createFolder(db, "Trips", 1);
    await updateLocal(db, ["chatgpt:a"], { folderId: folder.id });
    await deleteFolder(db, folder.id);
    expect((await getConversation(db, "chatgpt:a"))?.folderId).toBeNull();
    await addExcerpt(db, { site: "chatgpt", conversationId: "a", url: "https://chatgpt.com/c/a", pageTitle: "A", text: "tip", note: "" }, 5);
    expect((await listExcerpts(db, "chatgpt", "a")).map((e) => e.text)).toEqual(["tip"]);
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/lib/db.test.ts`
Expected: FAIL（模块不存在）。

- [ ] **Step 3: 实现**

`extension/src/lib/db.ts`：

```ts
import { openDB, type DBSchema, type IDBPDatabase } from "idb";
import type { RemoteAccount, RemoteBody, RemoteConversation, SiteId } from "../shared/types";
import { SITES } from "../sites/registry";

export interface Account { key: string; site: SiteId; remoteId: string; alias: string; lastSeen: number }
export interface LocalFields { folderId: string | null; tags: string[]; favorite: boolean; note: string }
export interface Conversation extends LocalFields {
  key: string; site: SiteId; account: string; id: string; title: string;
  createdAt: number; updatedAt: number; archived: boolean;
  bodyFetchedAt: number | null; bodyUpdatedAt: number | null; removedAt: number | null;
}
export interface StoredBody extends RemoteBody { key: string }
export interface Folder { id: string; name: string; createdAt: number }
export interface Excerpt { id: string; site: SiteId; conversationId: string | null; url: string; pageTitle: string; text: string; note: string; createdAt: number }

interface Schema extends DBSchema {
  accounts: { key: string; value: Account };
  conversations: { key: string; value: Conversation; indexes: { account: string } };
  bodies: { key: string; value: StoredBody };
  folders: { key: string; value: Folder };
  excerpts: { key: string; value: Excerpt; indexes: { conversation: [SiteId, string] } };
}
export type Db = IDBPDatabase<Schema>;

export function openDb(name = "stacker-web"): Promise<Db> {
  return openDB<Schema>(name, 1, {
    upgrade(db) {
      db.createObjectStore("accounts", { keyPath: "key" });
      db.createObjectStore("conversations", { keyPath: "key" }).createIndex("account", "account");
      db.createObjectStore("bodies", { keyPath: "key" });
      db.createObjectStore("folders", { keyPath: "id" });
      db.createObjectStore("excerpts", { keyPath: "id" }).createIndex("conversation", ["site", "conversationId"]);
    },
  });
}

export const conversationKey = (site: SiteId, id: string) => `${site}:${id}`;
export const accountKey = (site: SiteId, remoteId: string) => `${site}:${remoteId}`;
const newId = () => crypto.randomUUID();

export async function upsertAccount(db: Db, site: SiteId, remote: RemoteAccount, now: number): Promise<Account> {
  const key = accountKey(site, remote.remoteId);
  const existing = await db.get("accounts", key);
  if (existing) {
    const next = { ...existing, lastSeen: now };
    await db.put("accounts", next);
    return next;
  }
  const sameSite = (await db.getAll("accounts")).filter((a) => a.site === site).length;
  const account: Account = { key, site, remoteId: remote.remoteId, alias: sameSite ? `${SITES[site].label} ${sameSite + 1}` : SITES[site].label, lastSeen: now };
  await db.put("accounts", account);
  return account;
}

export const listAccounts = (db: Db) => db.getAll("accounts");

export async function renameAccount(db: Db, key: string, alias: string): Promise<void> {
  const a = await db.get("accounts", key);
  if (a) await db.put("accounts", { ...a, alias: alias.trim() || a.alias });
}

/** Refreshes titles and times, keeps local fields; a complete listing marks the rest removed. */
export async function mergeListing(db: Db, account: Account, items: RemoteConversation[], complete: boolean, now: number) {
  const tx = db.transaction("conversations", "readwrite");
  const counts = { added: 0, updated: 0, removed: 0 };
  const seen = new Set<string>();
  for (const item of items) {
    const key = conversationKey(account.site, item.id);
    seen.add(key);
    const old = await tx.store.get(key);
    if (!old) counts.added++;
    else if (old.updatedAt !== item.updatedAt || old.title !== item.title || old.archived !== item.archived || old.removedAt !== null) counts.updated++;
    await tx.store.put({
      folderId: null, tags: [], favorite: false, note: "", bodyFetchedAt: null, bodyUpdatedAt: null,
      ...old,
      key, site: account.site, account: account.key, id: item.id, title: item.title,
      createdAt: item.createdAt, updatedAt: item.updatedAt, archived: item.archived, removedAt: null,
    });
  }
  if (complete) {
    for (const c of await tx.store.index("account").getAll(account.key)) {
      if (!seen.has(c.key) && c.removedAt === null) {
        counts.removed++;
        await tx.store.put({ ...c, removedAt: now });
      }
    }
  }
  await tx.done;
  return counts;
}

export const listConversations = (db: Db) => db.getAll("conversations");
export const getConversation = (db: Db, key: string) => db.get("conversations", key);

export async function updateLocal(db: Db, keys: string[], patch: Partial<LocalFields>): Promise<void> {
  const tx = db.transaction("conversations", "readwrite");
  for (const key of keys) {
    const c = await tx.store.get(key);
    if (c) await tx.store.put({ ...c, ...patch });
  }
  await tx.done;
}

export async function addTag(db: Db, keys: string[], tag: string): Promise<void> {
  const clean = tag.trim();
  if (!clean) return;
  const tx = db.transaction("conversations", "readwrite");
  for (const key of keys) {
    const c = await tx.store.get(key);
    if (c && !c.tags.includes(clean)) await tx.store.put({ ...c, tags: [...c.tags, clean] });
  }
  await tx.done;
}

export async function putBody(db: Db, key: string, body: RemoteBody, now: number): Promise<void> {
  const tx = db.transaction(["bodies", "conversations"], "readwrite");
  await tx.objectStore("bodies").put({ ...body, key });
  const c = await tx.objectStore("conversations").get(key);
  if (c) await tx.objectStore("conversations").put({ ...c, bodyFetchedAt: now, bodyUpdatedAt: body.updatedAt });
  await tx.done;
}

export const getBody = (db: Db, key: string) => db.get("bodies", key);

export function bodyIsFresh(c: Conversation): boolean {
  return c.bodyUpdatedAt !== null && c.bodyUpdatedAt >= c.updatedAt;
}

export async function markRemoved(db: Db, key: string, now: number): Promise<void> {
  const c = await db.get("conversations", key);
  if (c) await db.put("conversations", { ...c, removedAt: now });
}

export const listFolders = (db: Db) => db.getAll("folders");

export async function createFolder(db: Db, name: string, now: number): Promise<Folder> {
  const folder = { id: newId(), name: name.trim(), createdAt: now };
  await db.put("folders", folder);
  return folder;
}

export async function renameFolder(db: Db, id: string, name: string): Promise<void> {
  const f = await db.get("folders", id);
  if (f && name.trim()) await db.put("folders", { ...f, name: name.trim() });
}

export async function deleteFolder(db: Db, id: string): Promise<void> {
  const tx = db.transaction(["folders", "conversations"], "readwrite");
  await tx.objectStore("folders").delete(id);
  for (const c of await tx.objectStore("conversations").getAll()) {
    if (c.folderId === id) await tx.objectStore("conversations").put({ ...c, folderId: null });
  }
  await tx.done;
}

export async function addExcerpt(db: Db, e: Omit<Excerpt, "id" | "createdAt">, now: number): Promise<Excerpt> {
  const excerpt = { ...e, id: newId(), createdAt: now };
  await db.put("excerpts", excerpt);
  return excerpt;
}

export async function listExcerpts(db: Db, site?: SiteId, conversationId?: string): Promise<Excerpt[]> {
  const all = site && conversationId ? await db.getAllFromIndex("excerpts", "conversation", [site, conversationId]) : await db.getAll("excerpts");
  return all.sort((a, b) => b.createdAt - a.createdAt);
}

export const deleteExcerpt = (db: Db, id: string) => db.delete("excerpts", id);
```

- [ ] **Step 4: 运行测试**

Run: `npx vitest run extension/src/lib/db.test.ts`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add extension/src/lib/db.ts extension/src/lib/db.test.ts
git commit -m "feat(extension): IndexedDB store for accounts, conversations, bodies, folders and excerpts"
```

---

### Task 7: 限速与刷新标题索引

**Files:**
- Create: `extension/src/lib/pacer.ts`, `extension/src/lib/pacer.test.ts`, `extension/src/lib/refresh.ts`, `extension/src/lib/refresh.test.ts`

**Interfaces:**
- Consumes: `SiteApi`（Task 5），db 函数（Task 6）。
- Produces:

```ts
// pacer.ts
export interface Pacer { wait(): Promise<void>; rateLimited(retryAfterMs: number | null): number; ok(): void }
export function createPacer(opts?: { gapMs?: number; maxBackoffMs?: number; sleep?: (ms: number) => Promise<void>; now?: () => number }): Pacer;
export const sleep: (ms: number, signal?: AbortSignal) => Promise<void>;

// refresh.ts
export interface RefreshResult { account: Account; added: number; updated: number; removed: number; total: number }
export function refreshIndex(api: SiteApi, db: Db, site: SiteId, pacer: Pacer, onPage: (total: number) => void, now?: () => number): Promise<RefreshResult>;
```

- [ ] **Step 1: 写失败的测试**

`extension/src/lib/pacer.test.ts`：

```ts
import { describe, expect, it } from "vitest";
import { createPacer } from "./pacer";

describe("pacer", () => {
  it("keeps a minimum gap and backs off on rate limits", async () => {
    let clock = 0;
    const slept: number[] = [];
    const p = createPacer({ gapMs: 1000, maxBackoffMs: 8000, now: () => clock, sleep: async (ms) => { slept.push(ms); clock += ms; } });
    await p.wait();
    clock += 200;
    await p.wait();
    expect(slept).toEqual([800]);
    expect(p.rateLimited(null)).toBe(2000);
    expect(p.rateLimited(null)).toBe(4000);
    expect(p.rateLimited(null)).toBe(8000);
    expect(p.rateLimited(null)).toBe(8000);
    expect(p.rateLimited(30000)).toBe(30000);
    p.ok();
    await p.wait();
    expect(slept.at(-1)).toBe(1000);
  });
});
```

`extension/src/lib/refresh.test.ts`：

```ts
import "fake-indexeddb/auto";
import { describe, expect, it } from "vitest";
import type { SiteApi } from "./siteClient";
import { listConversations, openDb } from "./db";
import { createPacer } from "./pacer";
import { refreshIndex } from "./refresh";

const conv = (id: string) => ({ id, title: id, createdAt: 1, updatedAt: 2, archived: false });

describe("refreshIndex", () => {
  it("walks every page, then marks the rest of that account removed", async () => {
    const db = await openDb("refresh-1");
    const pages: Record<string, { items: ReturnType<typeof conv>[]; next: string | null }> = {
      first: { items: [conv("a"), conv("b")], next: "2" },
      "2": { items: [conv("c")], next: null },
    };
    const api = {
      account: async () => ({ remoteId: "u", label: "ChatGPT" }),
      list: async (_site: string, cursor: string | null) => pages[cursor ?? "first"],
    } as unknown as SiteApi;
    const totals: number[] = [];
    const pacer = createPacer({ gapMs: 0, sleep: async () => {} });
    const result = await refreshIndex(api, db, "chatgpt", pacer, (n) => totals.push(n), () => 5);
    expect(result).toMatchObject({ added: 3, removed: 0, total: 3 });
    expect(totals).toEqual([2, 3]);
    pages.first = { items: [conv("a")], next: null };
    const again = await refreshIndex(api, db, "chatgpt", pacer, () => {}, () => 6);
    expect(again).toMatchObject({ removed: 2 });
    expect((await listConversations(db)).filter((c) => c.removedAt === 6).map((c) => c.id).sort()).toEqual(["b", "c"]);
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/lib/pacer.test.ts extension/src/lib/refresh.test.ts`
Expected: FAIL（模块不存在）。

- [ ] **Step 3: 实现**

`extension/src/lib/pacer.ts`：

```ts
import { SiteError } from "../shared/types";

export interface Pacer { wait(): Promise<void>; rateLimited(retryAfterMs: number | null): number; ok(): void }

export function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) return reject(new SiteError("E_CANCELLED"));
    const timer = setTimeout(resolve, ms);
    signal?.addEventListener("abort", () => { clearTimeout(timer); reject(new SiteError("E_CANCELLED")); }, { once: true });
  });
}

/** One request per `gapMs`; after a rate limit the gap doubles up to `maxBackoffMs` (or follows Retry-After). */
export function createPacer({ gapMs = 1500, maxBackoffMs = 60_000, sleep: nap = sleep, now = Date.now }: {
  gapMs?: number; maxBackoffMs?: number; sleep?: (ms: number) => Promise<void>; now?: () => number;
} = {}): Pacer {
  let last = -Infinity;
  let gap = gapMs;
  return {
    async wait() {
      const due = last + gap - now();
      if (due > 0) await nap(due);
      last = now();
    },
    rateLimited(retryAfterMs) {
      gap = retryAfterMs ?? Math.min(Math.max(gap, gapMs) * 2, maxBackoffMs);
      return gap;
    },
    ok() { gap = gapMs; },
  };
}
```

`extension/src/lib/refresh.ts`：

```ts
import type { SiteId } from "../shared/types";
import { mergeListing, upsertAccount, type Account, type Db } from "./db";
import type { Pacer } from "./pacer";
import type { SiteApi } from "./siteClient";

export interface RefreshResult { account: Account; added: number; updated: number; removed: number; total: number }

/** Re-lists every conversation of the signed-in account; only a finished walk marks missing ones removed. */
export async function refreshIndex(api: SiteApi, db: Db, site: SiteId, pacer: Pacer, onPage: (total: number) => void, now: () => number = Date.now): Promise<RefreshResult> {
  await pacer.wait();
  const account = await upsertAccount(db, site, await api.account(site), now());
  const items = [];
  let cursor: string | null = null;
  do {
    await pacer.wait();
    const page = await api.list(site, cursor);
    items.push(...page.items);
    onPage(items.length);
    cursor = page.next;
  } while (cursor);
  const counts = await mergeListing(db, account, items, true, now());
  return { account, ...counts, total: items.length };
}
```

- [ ] **Step 4: 运行测试**

Run: `npx vitest run extension/src/lib`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add extension/src/lib/pacer.ts extension/src/lib/pacer.test.ts extension/src/lib/refresh.ts extension/src/lib/refresh.test.ts
git commit -m "feat(extension): paced title index refresh"
```

---

### Task 8: 筛选与搜索

**Files:**
- Create: `extension/src/lib/search.ts`, `extension/src/lib/search.test.ts`

**Interfaces:**
- Consumes: `Conversation`、`StoredBody`（Task 6）。
- Produces:

```ts
export interface Filter {
  text: string; inBody: boolean; site: SiteId | ""; account: string;
  folder: string; // "" 全部, "none" 未归入, 或文件夹 id
  tag: string; from: number | null; to: number | null;
  body: "any" | "read" | "unread"; favorite: boolean; showRemoved: boolean;
}
export const EMPTY_FILTER: Filter;
export function bodyText(body: StoredBody): string; // lower-cased, for search
export function applyFilter(list: Conversation[], f: Filter, bodies: Map<string, string>): Conversation[]; // updatedAt desc
export function allTags(list: Conversation[]): string[];
```

- [ ] **Step 1: 写失败的测试**

`extension/src/lib/search.test.ts`：

```ts
import { describe, expect, it } from "vitest";
import type { Conversation } from "./db";
import { allTags, applyFilter, bodyText, EMPTY_FILTER } from "./search";

const c = (id: string, patch: Partial<Conversation> = {}): Conversation => ({
  key: `chatgpt:${id}`, site: "chatgpt", account: "chatgpt:u", id, title: id, createdAt: 0, updatedAt: 0, archived: false,
  folderId: null, tags: [], favorite: false, note: "", bodyFetchedAt: null, bodyUpdatedAt: null, removedAt: null, ...patch,
});

describe("search", () => {
  const list = [
    c("Kyoto trip", { updatedAt: 3, tags: ["travel"], favorite: true, bodyUpdatedAt: 3 }),
    c("Rust build", { updatedAt: 5, folderId: "f1", site: "claude", key: "claude:r", account: "claude:o" }),
    c("Old", { updatedAt: 1, removedAt: 9 }),
  ];
  const bodies = new Map([["chatgpt:Kyoto trip", bodyText({ key: "chatgpt:Kyoto trip", id: "k", title: "", updatedAt: 3, messages: [{ role: "assistant", text: "Temples in Higashiyama", at: null, attachments: [] }] })]]);

  it("sorts newest first and hides removed ones unless asked", () => {
    expect(applyFilter(list, EMPTY_FILTER, bodies).map((x) => x.id)).toEqual(["Rust build", "Kyoto trip"]);
    expect(applyFilter(list, { ...EMPTY_FILTER, showRemoved: true }, bodies)).toHaveLength(3);
  });
  it("matches titles, and bodies only when asked", () => {
    expect(applyFilter(list, { ...EMPTY_FILTER, text: "higashi" }, bodies)).toEqual([]);
    expect(applyFilter(list, { ...EMPTY_FILTER, text: "higashi", inBody: true }, bodies).map((x) => x.id)).toEqual(["Kyoto trip"]);
    expect(applyFilter(list, { ...EMPTY_FILTER, text: "RUST" }, bodies).map((x) => x.id)).toEqual(["Rust build"]);
  });
  it("filters by site, folder, tag, favorite, body state and time", () => {
    expect(applyFilter(list, { ...EMPTY_FILTER, site: "claude" }, bodies).map((x) => x.id)).toEqual(["Rust build"]);
    expect(applyFilter(list, { ...EMPTY_FILTER, folder: "none" }, bodies).map((x) => x.id)).toEqual(["Kyoto trip"]);
    expect(applyFilter(list, { ...EMPTY_FILTER, tag: "travel", favorite: true }, bodies)).toHaveLength(1);
    expect(applyFilter(list, { ...EMPTY_FILTER, body: "unread" }, bodies).map((x) => x.id)).toEqual(["Rust build"]);
    expect(applyFilter(list, { ...EMPTY_FILTER, from: 4 }, bodies).map((x) => x.id)).toEqual(["Rust build"]);
    expect(allTags(list)).toEqual(["travel"]);
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/lib/search.test.ts`
Expected: FAIL

- [ ] **Step 3: 实现**

`extension/src/lib/search.ts`：

```ts
import type { SiteId } from "../shared/types";
import { bodyIsFresh, type Conversation, type StoredBody } from "./db";

export interface Filter {
  text: string; inBody: boolean; site: SiteId | ""; account: string;
  folder: string; tag: string; from: number | null; to: number | null;
  body: "any" | "read" | "unread"; favorite: boolean; showRemoved: boolean;
}

export const EMPTY_FILTER: Filter = {
  text: "", inBody: false, site: "", account: "", folder: "", tag: "", from: null, to: null,
  body: "any", favorite: false, showRemoved: false,
};

export function bodyText(body: StoredBody): string {
  return body.messages.map((m) => m.text).join("\n").toLowerCase();
}

export function applyFilter(list: Conversation[], f: Filter, bodies: Map<string, string>): Conversation[] {
  const needle = f.text.trim().toLowerCase();
  return list
    .filter((c) => f.showRemoved || c.removedAt === null)
    .filter((c) => !f.site || c.site === f.site)
    .filter((c) => !f.account || c.account === f.account)
    .filter((c) => !f.folder || (f.folder === "none" ? c.folderId === null : c.folderId === f.folder))
    .filter((c) => !f.tag || c.tags.includes(f.tag))
    .filter((c) => !f.favorite || c.favorite)
    .filter((c) => f.from === null || c.updatedAt >= f.from)
    .filter((c) => f.to === null || c.updatedAt <= f.to)
    .filter((c) => f.body === "any" || (f.body === "read") === bodyIsFresh(c))
    .filter((c) => !needle || c.title.toLowerCase().includes(needle) || c.note.toLowerCase().includes(needle)
      || (f.inBody && (bodies.get(c.key) ?? "").includes(needle)))
    .sort((a, b) => b.updatedAt - a.updatedAt);
}

export function allTags(list: Conversation[]): string[] {
  return [...new Set(list.flatMap((c) => c.tags))].sort();
}
```

- [ ] **Step 4: 运行测试**

Run: `npx vitest run extension/src/lib/search.test.ts`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add extension/src/lib/search.ts extension/src/lib/search.test.ts
git commit -m "feat(extension): conversation filters and title/body search"
```

---

### Task 9: 导出 Markdown

**Files:**
- Create: `extension/src/lib/markdown.ts`, `extension/src/lib/markdown.test.ts`, `extension/src/lib/download.ts`

**Interfaces:**
- Consumes: `Conversation`、`StoredBody`、`Account`（Task 6），`SITES`（Task 4）。
- Produces:

```ts
export type ExportMode = "slim" | "full";
export function toMarkdown(c: Conversation, accountAlias: string, body: StoredBody, mode: ExportMode, url: string): string;
export function exportFileName(c: Conversation, ext: "md" | "json"): string; // `<site>/<yyyy-mm-dd> <title> <id8>.<ext>`
// download.ts
export const EXPORT_ROOT = "Stacker 网页对话";
export function saveFile(path: string, text: string, mime: string): Promise<void>;
```

- [ ] **Step 1: 写失败的测试**

`extension/src/lib/markdown.test.ts`：

```ts
import { describe, expect, it } from "vitest";
import type { Conversation, StoredBody } from "./db";
import { exportFileName, toMarkdown } from "./markdown";

const conv: Conversation = {
  key: "chatgpt:abcdef123456", site: "chatgpt", account: "chatgpt:u", id: "abcdef123456", title: "Kyoto: plan/2",
  createdAt: Date.UTC(2026, 8, 1), updatedAt: Date.UTC(2026, 8, 2), archived: false,
  folderId: null, tags: ["travel"], favorite: false, note: "", bodyFetchedAt: 1, bodyUpdatedAt: 1, removedAt: null,
};
const body: StoredBody = {
  key: conv.key, id: conv.id, title: conv.title, updatedAt: 1,
  messages: [
    { role: "user", text: "Where?", at: Date.UTC(2026, 8, 1, 1), attachments: ["map.png"] },
    { role: "tool", text: "print(1)", at: null, attachments: [] },
    { role: "assistant", text: "Kyoto.", at: null, attachments: [] },
  ],
};

describe("markdown export", () => {
  it("slim keeps only user and assistant text under a header", () => {
    const md = toMarkdown(conv, "Work", body, "slim", "https://chatgpt.com/c/abcdef123456");
    expect(md).toContain("# Kyoto: plan/2");
    expect(md).toContain("- 站点：ChatGPT");
    expect(md).toContain("- 账号：Work");
    expect(md).toContain("- 链接：https://chatgpt.com/c/abcdef123456");
    expect(md).toContain("## 用户\n\nWhere?");
    expect(md).toContain("## 助手\n\nKyoto.");
    expect(md).not.toContain("print(1)");
    expect(md).not.toContain("map.png");
  });
  it("full adds tool messages, attachments and times", () => {
    const md = toMarkdown(conv, "Work", body, "full", "u");
    expect(md).toContain("## 工具");
    expect(md).toContain("附件：map.png");
    expect(md).toContain("2026-09-01");
  });
  it("makes a safe, dated file name per site", () => {
    expect(exportFileName(conv, "md")).toBe("chatgpt/2026-09-02 Kyoto plan 2 abcdef12.md");
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/lib/markdown.test.ts`
Expected: FAIL

- [ ] **Step 3: 实现**

`extension/src/lib/markdown.ts`：

```ts
import type { Role } from "../shared/types";
import { SITES } from "../sites/registry";
import type { Conversation, StoredBody } from "./db";

export type ExportMode = "slim" | "full";

const ROLE_LABEL: Record<Role, string> = { user: "用户", assistant: "助手", system: "系统", tool: "工具" };
const day = (ms: number) => new Date(ms).toISOString().slice(0, 10);
const stamp = (ms: number) => new Date(ms).toISOString().replace("T", " ").slice(0, 16);

export function toMarkdown(c: Conversation, accountAlias: string, body: StoredBody, mode: ExportMode, url: string): string {
  const head = [
    `# ${c.title || c.id}`,
    "",
    `- 站点：${SITES[c.site].label}`,
    `- 账号：${accountAlias}`,
    `- 链接：${url}`,
    `- 创建：${stamp(c.createdAt)}`,
    `- 更新：${stamp(c.updatedAt)}`,
    ...(c.tags.length ? [`- 标签：${c.tags.join("、")}`] : []),
    ...(c.note ? [`- 备注：${c.note}`] : []),
  ];
  const messages = body.messages
    .filter((m) => mode === "full" || m.role === "user" || m.role === "assistant")
    .map((m) => {
      const title = `## ${ROLE_LABEL[m.role]}${mode === "full" && m.at !== null ? ` · ${stamp(m.at)}` : ""}`;
      const files = mode === "full" && m.attachments.length ? `\n\n附件：${m.attachments.join("、")}` : "";
      return `${title}\n\n${m.text}${files}`;
    });
  return [...head, "", ...messages.flatMap((m) => [m, ""])].join("\n");
}

export function exportFileName(c: Conversation, ext: "md" | "json"): string {
  const title = (c.title || "untitled").replace(/[\\/:*?"<>|\s]+/g, " ").trim().slice(0, 60);
  return `${c.site}/${day(c.updatedAt)} ${title} ${c.id.slice(0, 8)}.${ext}`;
}
```

`extension/src/lib/download.ts`：

```ts
export const EXPORT_ROOT = "Stacker 网页对话";

/** Saves into the browser's download folder under `Stacker 网页对话/`, never asking where. */
export async function saveFile(path: string, text: string, mime: string): Promise<void> {
  const url = URL.createObjectURL(new Blob([text], { type: `${mime};charset=utf-8` }));
  try {
    await chrome.downloads.download({ url, filename: `${EXPORT_ROOT}/${path}`, conflictAction: "uniquify", saveAs: false });
  } finally {
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
  }
}
```

- [ ] **Step 4: 运行测试**

Run: `npx vitest run extension/src/lib/markdown.test.ts`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add extension/src/lib/markdown.ts extension/src/lib/markdown.test.ts extension/src/lib/download.ts
git commit -m "feat(extension): slim and full Markdown export"
```

---

### Task 10: 删除任务

**Files:**
- Create: `extension/src/lib/deleteJob.ts`, `extension/src/lib/deleteJob.test.ts`

**Interfaces:**
- Consumes: `SiteApi`（Task 5），db（Task 6），`Pacer`（Task 7），`toMarkdown`/`exportFileName`（Task 9）。
- Produces:

```ts
export type DeleteMode = "slim" | "full" | "direct";
export type ItemStatus = "pending" | "done" | "failed" | "skipped";
export interface ItemResult { key: string; title: string; status: ItemStatus; error: string }
export interface DeleteDeps {
  api: SiteApi; db: Db; pacer: Pacer; now: () => number;
  save: (path: string, text: string, mime: string) => Promise<void>;
  aliasOf: (accountKey: string) => string;
}
export function runDeleteJob(items: Conversation[], mode: DeleteMode, deps: DeleteDeps, signal: AbortSignal, onProgress: (results: ItemResult[]) => void): Promise<ItemResult[]>;
```

行为：
- 按账号分组；每组开始前 `api.account(site)`，若 `accountKey(site, remoteId) !== 组账号` → 该组全部 `skipped`，`error = "E_ACCOUNT"`。
- 非直接删除：正文不新鲜就 `api.read` 并 `putBody`；`slim` 保存精简 `.md`；`full` 保存完整 `.md` 与原始 `.json`；导出失败则该条 `failed`，**不删除**。
- 然后 `api.remove`，成功后 `markRemoved`。
- `E_RATE`：`pacer.rateLimited`，睡眠后重试同一条，最多 5 次；仍失败则停止任务。
- `E_BROKEN`、`E_AUTH`、`E_NO_TAB`、`E_NO_AGENT`：停止任务，剩余 `skipped` 并写入该错误码。
- `E_NOT_FOUND` 删除时视为已删除（`done`，并 `markRemoved`）。
- 中止：剩余 `skipped`，`error = "E_CANCELLED"`。

- [ ] **Step 1: 写失败的测试**

`extension/src/lib/deleteJob.test.ts`：

```ts
import "fake-indexeddb/auto";
import { describe, expect, it, vi } from "vitest";
import { SiteError } from "../shared/types";
import { getConversation, listConversations, mergeListing, openDb, upsertAccount, type Db } from "./db";
import { runDeleteJob, type DeleteDeps } from "./deleteJob";
import { createPacer } from "./pacer";
import type { SiteApi } from "./siteClient";

let n = 0;
async function setup(ids: string[], remoteId = "u") {
  const db = await openDb(`del-${n++}`);
  const account = await upsertAccount(db, "chatgpt", { remoteId: "u", label: "ChatGPT" }, 1);
  await mergeListing(db, account, ids.map((id) => ({ id, title: id, createdAt: 1, updatedAt: 2, archived: false })), true, 1);
  const api = {
    account: vi.fn(async () => ({ remoteId, label: "ChatGPT" })),
    read: vi.fn(async (_s: string, id: string) => ({ id, title: id, updatedAt: 2, messages: [{ role: "user", text: `q ${id}`, at: null, attachments: [] }] })),
    remove: vi.fn(async () => {}),
  } as unknown as SiteApi;
  const saved: string[] = [];
  const deps: DeleteDeps = {
    api, db, now: () => 50, pacer: createPacer({ gapMs: 0, sleep: async () => {} }),
    save: async (path) => { saved.push(path); }, aliasOf: () => "Work",
  };
  return { db, api, deps, saved, items: await listConversations(db) };
}
const run = (items: Awaited<ReturnType<typeof setup>>["items"], mode: "slim" | "full" | "direct", deps: DeleteDeps, signal = new AbortController().signal) =>
  runDeleteJob(items, mode, deps, signal, () => {});

async function removed(db: Db, key: string) { return (await getConversation(db, key))?.removedAt; }

describe("delete job", () => {
  it("exports slim Markdown before deleting each conversation", async () => {
    const { db, api, deps, saved, items } = await setup(["a", "b"]);
    const results = await run(items, "slim", deps);
    expect(results.map((r) => r.status)).toEqual(["done", "done"]);
    expect(saved).toHaveLength(2);
    expect(saved.every((p) => p.endsWith(".md"))).toBe(true);
    expect(api.remove).toHaveBeenCalledTimes(2);
    expect(await removed(db, "chatgpt:a")).toBe(50);
  });
  it("writes Markdown and raw JSON for a full backup, nothing for direct", async () => {
    const full = await setup(["a"]);
    await run(full.items, "full", full.deps);
    expect(full.saved.map((p) => p.split(".").pop())).toEqual(["md", "json"]);
    const direct = await setup(["a"]);
    await run(direct.items, "direct", direct.deps);
    expect(direct.saved).toEqual([]);
    expect(direct.api.read).not.toHaveBeenCalled();
  });
  it("never deletes when the export fails", async () => {
    const { deps, api, items } = await setup(["a"]);
    deps.save = async () => { throw new Error("disk full"); };
    const [r] = await run(items, "slim", deps);
    expect(r.status).toBe("failed");
    expect(api.remove).not.toHaveBeenCalled();
  });
  it("skips an account that is not the one signed in", async () => {
    const { deps, api, items } = await setup(["a"], "someone-else");
    const [r] = await run(items, "direct", deps);
    expect([r.status, r.error]).toEqual(["skipped", "E_ACCOUNT"]);
    expect(api.remove).not.toHaveBeenCalled();
  });
  it("retries after a rate limit and stops the job when the site changed", async () => {
    const { deps, api, items } = await setup(["a", "b", "c"]);
    let calls = 0;
    (api.remove as ReturnType<typeof vi.fn>).mockImplementation(async () => {
      calls++;
      if (calls === 1) throw new SiteError("E_RATE", "", 10);
      if (calls === 3) throw new SiteError("E_BROKEN", "page.items");
    });
    const results = await run(items, "direct", deps);
    expect(results.map((r) => [r.status, r.error])).toEqual([["done", ""], ["failed", "E_BROKEN"], ["skipped", "E_BROKEN"]]);
  });
  it("treats an already deleted conversation as done and honours abort", async () => {
    const gone = await setup(["a"]);
    (gone.api.remove as ReturnType<typeof vi.fn>).mockRejectedValue(new SiteError("E_NOT_FOUND"));
    expect((await run(gone.items, "direct", gone.deps))[0].status).toBe("done");
    const { deps, items } = await setup(["a", "b"]);
    const controller = new AbortController();
    controller.abort();
    expect((await run(items, "direct", deps, controller.signal)).map((r) => r.error)).toEqual(["E_CANCELLED", "E_CANCELLED"]);
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/lib/deleteJob.test.ts`
Expected: FAIL

- [ ] **Step 3: 实现**

`extension/src/lib/deleteJob.ts`：

```ts
import { SiteError, type ErrorCode } from "../shared/types";
import { conversationUrl } from "../sites/registry";
import { accountKey, bodyIsFresh, getBody, markRemoved, putBody, type Conversation, type Db, type StoredBody } from "./db";
import { exportFileName, toMarkdown } from "./markdown";
import { sleep, type Pacer } from "./pacer";
import type { SiteApi } from "./siteClient";

export type DeleteMode = "slim" | "full" | "direct";
export type ItemStatus = "pending" | "done" | "failed" | "skipped";
export interface ItemResult { key: string; title: string; status: ItemStatus; error: string }
export interface DeleteDeps {
  api: SiteApi; db: Db; pacer: Pacer; now: () => number;
  save: (path: string, text: string, mime: string) => Promise<void>;
  aliasOf: (accountKey: string) => string;
}

const STOP: ErrorCode[] = ["E_BROKEN", "E_AUTH", "E_NO_TAB", "E_NO_AGENT"];
const MAX_RATE_RETRIES = 5;
const code = (e: unknown): string => (e instanceof SiteError ? e.code : "E_NET");

export async function runDeleteJob(items: Conversation[], mode: DeleteMode, deps: DeleteDeps, signal: AbortSignal, onProgress: (results: ItemResult[]) => void): Promise<ItemResult[]> {
  const results: ItemResult[] = items.map((c) => ({ key: c.key, title: c.title, status: "pending", error: "" }));
  const report = () => onProgress(results.map((r) => ({ ...r })));
  const skipRest = (from: number, error: string) => {
    for (let i = from; i < results.length; i++) if (results[i].status === "pending") results[i] = { ...results[i], status: "skipped", error };
  };

  /** Retries rate limits; any other error is thrown. */
  async function paced<T>(task: () => Promise<T>): Promise<T> {
    for (let attempt = 0; ; attempt++) {
      await deps.pacer.wait();
      try {
        const value = await task();
        deps.pacer.ok();
        return value;
      } catch (e) {
        if (!(e instanceof SiteError) || e.code !== "E_RATE" || attempt >= MAX_RATE_RETRIES) throw e;
        await sleep(deps.pacer.rateLimited(e.retryAfterMs), signal);
      }
    }
  }

  async function exportOne(c: Conversation): Promise<void> {
    let body: StoredBody | undefined = bodyIsFresh(c) ? await getBody(deps.db, c.key) : undefined;
    if (!body) {
      const fresh = await paced(() => deps.api.read(c.site, c.id));
      await putBody(deps.db, c.key, fresh, deps.now());
      body = { ...fresh, key: c.key };
    }
    const url = conversationUrl(c.site, c.id);
    const alias = deps.aliasOf(c.account);
    await deps.save(exportFileName(c, "md"), toMarkdown(c, alias, body, mode === "full" ? "full" : "slim", url), "text/markdown");
    if (mode === "full") await deps.save(exportFileName(c, "json"), JSON.stringify(body, null, 2), "application/json");
  }

  const checked = new Map<string, boolean>();
  for (let i = 0; i < items.length; i++) {
    if (signal.aborted) { skipRest(i, "E_CANCELLED"); break; }
    const c = items[i];
    try {
      if (!checked.has(c.account)) {
        const remote = await paced(() => deps.api.account(c.site));
        checked.set(c.account, accountKey(c.site, remote.remoteId) === c.account);
      }
      if (!checked.get(c.account)) {
        results[i] = { ...results[i], status: "skipped", error: "E_ACCOUNT" };
        report();
        continue;
      }
      if (mode !== "direct") {
        try { await exportOne(c); } catch (e) {
          if (STOP.includes(code(e) as ErrorCode) || code(e) === "E_CANCELLED") throw e;
          results[i] = { ...results[i], status: "failed", error: e instanceof Error ? e.message : String(e) };
          report();
          continue;
        }
      }
      try {
        await paced(() => deps.api.remove(c.site, c.id));
      } catch (e) {
        if (code(e) !== "E_NOT_FOUND") throw e;
      }
      await markRemoved(deps.db, c.key, deps.now());
      results[i] = { ...results[i], status: "done", error: "" };
    } catch (e) {
      const err = code(e);
      if (err === "E_CANCELLED") { skipRest(i, "E_CANCELLED"); report(); break; }
      results[i] = { ...results[i], status: "failed", error: err };
      if (STOP.includes(err as ErrorCode) || err === "E_RATE") { skipRest(i + 1, err); report(); break; }
    }
    report();
  }
  report();
  return results;
}
```

- [ ] **Step 4: 运行测试**

Run: `npx vitest run extension/src/lib/deleteJob.test.ts`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add extension/src/lib/deleteJob.ts extension/src/lib/deleteJob.test.ts
git commit -m "feat(extension): paced delete job with export-first modes"
```

---

### Task 11: 管理页

管理页布局（`manage.html`，整页）：

- 顶栏：插件名；站点切换（全部 / ChatGPT / Claude）；账号下拉（该站点已知账号，可改备注名）；「刷新列表」按钮（对当前站点调用 `refreshIndex`，显示已读取条数）；打开 ChatGPT / Claude 的链接。
- 左栏：文件夹列表（全部、未归入、各文件夹，新建 / 重命名 / 删除）、标签列表。
- 中栏：筛选栏（搜索框、「搜索正文」勾选、正文状态、收藏、时间范围、显示已删除），列表（勾选、标题、站点、账号备注名、更新时间、标签、正文已读标记），「本页全选」与「选中全部结果（N）」，批量操作：移到文件夹、加标签、收藏、导出（精简 / 完整）、删除…。
- 右栏：选中对话的详情：标题、链接（打开原对话）、文件夹、标签、收藏、备注；「读取正文」/「重新读取」；正文消息列表；该对话的摘录。
- 错误提示用 `ERROR_TEXT` 映射错误码：`E_NO_TAB`「请先在浏览器中打开并登录该网站」，`E_NO_AGENT`「请刷新该网站页面后重试」，`E_AUTH`「该网站未登录或登录已过期」，`E_BROKEN`「该网站接口已变化，已停止操作，等待插件更新」，`E_RATE`「请求过于频繁，稍后再试」，`E_ACCOUNT`「不是当前登录的账号，已跳过」，`E_CANCELLED`「已中止」，`E_NOT_FOUND`「对话不存在」，`E_HTTP`/`E_NET`「网络或网站错误」。

**Files:**
- Create: `extension/src/ui/style.css`, `extension/src/ui/errors.ts`, `extension/src/ui/manage/App.tsx`, `extension/src/ui/manage/Filters.tsx`, `extension/src/ui/manage/ConversationList.tsx`, `extension/src/ui/manage/Detail.tsx`, `extension/src/ui/manage/DeleteDialog.tsx`, `extension/src/ui/manage/DeleteDialog.test.tsx`, `extension/src/ui/manage/ConversationList.test.tsx`
- Modify: `extension/src/ui/manage/main.tsx`, `extension/src/i18n.ts`

**Interfaces:**
- Consumes: Task 5–10 全部。
- Produces: `ERROR_TEXT: Record<string, string>` 与 `errorText(e: unknown): string`（`ui/errors.ts`，Task 12 复用）；`DeleteDialog` props：

```ts
{ items: Conversation[]; currentAccounts: Set<string>; onRun: (mode: DeleteMode, signal: AbortSignal, onProgress: (r: ItemResult[]) => void) => Promise<ItemResult[]>; onClose: (changed: boolean) => void }
```

`ConversationList` props：

```ts
{ items: Conversation[]; aliasOf: (key: string) => string; selected: Set<string>; onSelect: (keys: string[], on: boolean) => void; onOpen: (c: Conversation) => void; active: string | null }
```

- [ ] **Step 1: 写失败的测试**

`extension/src/ui/manage/DeleteDialog.test.tsx`：

```tsx
// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Conversation } from "../../lib/db";
import { DeleteDialog } from "./DeleteDialog";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const conv = (id: string, account = "chatgpt:u"): Conversation => ({
  key: `chatgpt:${id}`, site: "chatgpt", account, id, title: id, createdAt: 0, updatedAt: 0, archived: false,
  folderId: null, tags: [], favorite: false, note: "", bodyFetchedAt: null, bodyUpdatedAt: null, removedAt: null,
});
let host: HTMLDivElement;
afterEach(() => host?.remove());

function mount(onRun = vi.fn(async () => [])) {
  host = document.createElement("div");
  document.body.append(host);
  act(() => createRoot(host).render(<DeleteDialog items={[conv("a"), conv("b", "chatgpt:other")]} currentAccounts={new Set(["chatgpt:u"])} onRun={onRun} onClose={() => {}} />));
  return onRun;
}
const button = (text: string) => [...host.querySelectorAll("button")].find((b) => b.textContent?.includes(text))!;

describe("DeleteDialog", () => {
  it("defaults to slim export and warns about other accounts", () => {
    mount();
    expect((host.querySelector("input[value=slim]") as HTMLInputElement).checked).toBe(true);
    expect(host.textContent).toContain("1 条不属于当前登录的账号");
  });
  it("asks a second time before deleting without a copy", async () => {
    const onRun = mount();
    act(() => (host.querySelector("input[value=direct]") as HTMLInputElement).click());
    await act(async () => button("删除 2 条").click());
    expect(onRun).not.toHaveBeenCalled();
    expect(host.textContent).toContain("不会留下任何副本");
    await act(async () => button("确认直接删除").click());
    expect(onRun).toHaveBeenCalledWith("direct", expect.any(AbortSignal), expect.any(Function));
  });
});
```

`extension/src/ui/manage/ConversationList.test.tsx`：

```tsx
// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import type { Conversation } from "../../lib/db";
import { ConversationList } from "./ConversationList";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const conv = (id: string): Conversation => ({
  key: `claude:${id}`, site: "claude", account: "claude:o", id, title: `T ${id}`, createdAt: 0, updatedAt: 0, archived: false,
  folderId: null, tags: ["x"], favorite: false, note: "", bodyFetchedAt: 1, bodyUpdatedAt: 0, removedAt: null,
});

describe("ConversationList", () => {
  it("selects the page and every result", () => {
    const host = document.createElement("div");
    const onSelect = vi.fn();
    const items = Array.from({ length: 120 }, (_, i) => conv(String(i)));
    act(() => createRoot(host).render(<ConversationList items={items} aliasOf={() => "Personal"} selected={new Set()} onSelect={onSelect} onOpen={() => {}} active={null} />));
    expect(host.querySelectorAll("li")).toHaveLength(50);
    act(() => [...host.querySelectorAll("button")].find((b) => b.textContent?.includes("选中全部结果"))!.click());
    expect(onSelect).toHaveBeenCalledWith(items.map((c) => c.key), true);
    expect(host.textContent).toContain("Personal");
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/ui`
Expected: FAIL（组件不存在）。

- [ ] **Step 3: 实现错误文案与样式**

`extension/src/ui/errors.ts`：

```ts
import { SiteError } from "../shared/types";
import { t } from "../i18n";

export const ERROR_TEXT: Record<string, string> = {
  E_NO_TAB: "请先在浏览器中打开并登录该网站",
  E_NO_AGENT: "请刷新该网站页面后重试",
  E_AUTH: "该网站未登录或登录已过期",
  E_BROKEN: "该网站接口已变化，已停止操作，等待插件更新",
  E_RATE: "请求过于频繁，稍后再试",
  E_ACCOUNT: "不是当前登录的账号，已跳过",
  E_CANCELLED: "已中止",
  E_NOT_FOUND: "对话不存在",
  E_HTTP: "网络或网站错误",
  E_NET: "网络或网站错误",
};

export function errorText(e: unknown): string {
  const code = e instanceof SiteError ? e.code : typeof e === "string" ? e : "";
  return ERROR_TEXT[code] ? t(ERROR_TEXT[code]) : e instanceof Error ? e.message : String(e);
}
```

注意：`ERROR_TEXT` 的值经 `t()` 动态翻译，i18n 测试只扫描字面量 `t("…")`，因此这些值也必须手动加入 `EN`（Step 5 一并加入）。

`extension/src/ui/style.css`（与 Stacker 暗色风格一致、支持浅色）：

```css
:root { --bg:#14171c; --panel:#1b1f26; --bd:#2a303a; --tx:#e6e8eb; --mut:#8a93a0; --acc:#f5821f; --red:#e5534b; --yel:#d9a441; color-scheme: dark; }
@media (prefers-color-scheme: light) { :root { --bg:#f6f7f9; --panel:#fff; --bd:#dde1e6; --tx:#1f2937; --mut:#6b7380; color-scheme: light; } }
* { box-sizing: border-box; }
body { margin:0; background:var(--bg); color:var(--tx); font:13px/1.5 system-ui, "Microsoft YaHei", sans-serif; }
button { font:inherit; color:var(--tx); background:var(--panel); border:1px solid var(--bd); border-radius:6px; padding:4px 10px; cursor:pointer; }
button:disabled { opacity:.5; cursor:default; }
button.primary { background:var(--acc); border-color:var(--acc); color:#fff; }
button.danger { background:var(--red); border-color:var(--red); color:#fff; }
input, select, textarea { font:inherit; color:var(--tx); background:var(--bg); border:1px solid var(--bd); border-radius:6px; padding:4px 8px; }
.layout { display:grid; grid-template-columns:200px 1fr 380px; grid-template-rows:auto 1fr; height:100vh; }
.top { grid-column:1/-1; display:flex; gap:8px; align-items:center; padding:8px 12px; border-bottom:1px solid var(--bd); flex-wrap:wrap; }
.side, .main, .detail { overflow:auto; padding:10px 12px; }
.side { border-right:1px solid var(--bd); }
.detail { border-left:1px solid var(--bd); }
.filters { display:flex; gap:6px; flex-wrap:wrap; align-items:center; margin-bottom:8px; }
.list { list-style:none; margin:0; padding:0; }
.list li { display:grid; grid-template-columns:auto 1fr auto; gap:8px; align-items:center; padding:6px 4px; border-bottom:1px solid var(--bd); cursor:pointer; }
.list li.active { background:rgba(245,130,31,.12); }
.mut { color:var(--mut); font-size:12px; }
.chip { display:inline-block; padding:0 6px; border:1px solid var(--bd); border-radius:999px; font-size:11px; margin-right:4px; }
.warn { color:var(--yel); }
.err { color:var(--red); }
.mask { position:fixed; inset:0; background:rgba(0,0,0,.45); display:grid; place-items:center; z-index:10; }
.dialog { background:var(--panel); border:1px solid var(--bd); border-radius:10px; padding:16px; width:min(560px, 92vw); max-height:86vh; overflow:auto; }
.msg { border-bottom:1px solid var(--bd); padding:8px 0; white-space:pre-wrap; }
.row { display:flex; gap:8px; align-items:center; flex-wrap:wrap; }
```

- [ ] **Step 4: 实现组件**

`extension/src/ui/manage/ConversationList.tsx`：

```tsx
import { useState } from "react";
import { t } from "../../i18n";
import { bodyIsFresh, type Conversation } from "../../lib/db";
import { SITES } from "../../sites/registry";

const PAGE = 50;

export function ConversationList({ items, aliasOf, selected, onSelect, onOpen, active }: {
  items: Conversation[]; aliasOf: (key: string) => string; selected: Set<string>;
  onSelect: (keys: string[], on: boolean) => void; onOpen: (c: Conversation) => void; active: string | null;
}) {
  const [page, setPage] = useState(0);
  const pages = Math.max(1, Math.ceil(items.length / PAGE));
  const current = Math.min(page, pages - 1);
  const shown = items.slice(current * PAGE, current * PAGE + PAGE);
  const allShown = shown.length > 0 && shown.every((c) => selected.has(c.key));
  return <>
    <div className="row">
      <label><input type="checkbox" checked={allShown} onChange={(e) => onSelect(shown.map((c) => c.key), e.target.checked)} /> {t("本页")}</label>
      <button disabled={!items.length} onClick={() => onSelect(items.map((c) => c.key), true)}>{t("选中全部结果")}（{items.length}）</button>
      {selected.size > 0 && <button onClick={() => onSelect([...selected], false)}>{t("取消选择")}</button>}
      <span className="mut">{t("已选")} {selected.size}</span>
    </div>
    <ul className="list">
      {shown.map((c) => <li key={c.key} className={active === c.key ? "active" : ""} onClick={() => onOpen(c)}>
        <input type="checkbox" checked={selected.has(c.key)} onClick={(e) => e.stopPropagation()} onChange={(e) => onSelect([c.key], e.target.checked)} />
        <div>
          <div>{c.favorite ? "★ " : ""}{c.title || t("（无标题）")}{c.removedAt !== null && <span className="mut"> · {t("已删除")}</span>}</div>
          <div className="mut">{SITES[c.site].label} · {aliasOf(c.account)} · {new Date(c.updatedAt).toLocaleString()}
            {bodyIsFresh(c) && <> · {t("正文已读")}</>}</div>
          <div>{c.tags.map((tag) => <span key={tag} className="chip">{tag}</span>)}</div>
        </div>
        <span className="mut">{c.archived ? t("已归档") : ""}</span>
      </li>)}
    </ul>
    {pages > 1 && <div className="row">
      <button disabled={current === 0} onClick={() => setPage(current - 1)}>‹</button>
      <span className="mut">{current + 1} / {pages}</span>
      <button disabled={current >= pages - 1} onClick={() => setPage(current + 1)}>›</button>
    </div>}
  </>;
}
```

`extension/src/ui/manage/DeleteDialog.tsx`：

```tsx
import { useRef, useState } from "react";
import { t } from "../../i18n";
import type { Conversation } from "../../lib/db";
import type { DeleteMode, ItemResult } from "../../lib/deleteJob";
import { errorText } from "../errors";

const MODES: { value: DeleteMode; label: string; hint: string }[] = [
  { value: "slim", label: "精简导出后删除", hint: "把用户和助手的正文存成 Markdown，再删除网站上的对话。" },
  { value: "full", label: "完整备份后删除", hint: "保存完整 Markdown 和原始数据，再删除网站上的对话。" },
  { value: "direct", label: "直接删除", hint: "不留任何副本。" },
];

export function DeleteDialog({ items, currentAccounts, onRun, onClose }: {
  items: Conversation[]; currentAccounts: Set<string>;
  onRun: (mode: DeleteMode, signal: AbortSignal, onProgress: (r: ItemResult[]) => void) => Promise<ItemResult[]>;
  onClose: (changed: boolean) => void;
}) {
  const [mode, setMode] = useState<DeleteMode>("slim");
  const [confirming, setConfirming] = useState(false);
  const [results, setResults] = useState<ItemResult[] | null>(null);
  const [running, setRunning] = useState(false);
  const controller = useRef<AbortController | null>(null);
  const others = items.filter((c) => !currentAccounts.has(c.account)).length;

  async function start() {
    if (mode === "direct" && !confirming) { setConfirming(true); return; }
    controller.current = new AbortController();
    setRunning(true);
    try { setResults(await onRun(mode, controller.current.signal, setResults)); } finally { setRunning(false); }
  }

  const done = results?.filter((r) => r.status === "done").length ?? 0;
  return <div className="mask"><div className="dialog" role="dialog" aria-modal="true">
    <h3>{t("删除对话")}</h3>
    {!results && <>
      <p>{t("将删除")} {items.length} {t("条对话。")}</p>
      {others > 0 && <p className="warn">{others} {t("条不属于当前登录的账号，会被跳过。请先在网站上切换到对应账号。")}</p>}
      {MODES.map((m) => <label key={m.value} style={{ display: "block", margin: "6px 0" }}>
        <input type="radio" name="mode" value={m.value} checked={mode === m.value} onChange={() => { setMode(m.value); setConfirming(false); }} /> <b>{t(m.label)}</b>
        <div className="mut">{t(m.hint)}</div>
      </label>)}
      {confirming && <p className="err">{t("直接删除不会留下任何副本，删除后无法恢复。确定继续吗？")}</p>}
      <div className="row">
        <button onClick={() => onClose(false)}>{t("取消")}</button>
        <button className="danger" onClick={() => void start()}>{confirming ? t("确认直接删除") : `${t("删除")} ${items.length} ${t("条")}`}</button>
      </div>
    </>}
    {results && <>
      <p>{running ? t("正在删除，请不要关闭此页…") : t("已完成")} {done} / {results.length}</p>
      <ul className="list">{results.map((r) => <li key={r.key} style={{ cursor: "default" }}>
        <span>{r.status === "done" ? "✓" : r.status === "pending" ? "…" : "✗"}</span>
        <span>{r.title}</span>
        <span className="mut">{r.error ? errorText(r.error) : ""}</span>
      </li>)}</ul>
      <div className="row">
        {running && <button onClick={() => controller.current?.abort()}>{t("中止")}</button>}
        <button className="primary" disabled={running} onClick={() => onClose(true)}>{t("完成")}</button>
      </div>
    </>}
  </div></div>;
}
```

`extension/src/ui/manage/Filters.tsx`：

```tsx
import { t } from "../../i18n";
import type { Filter } from "../../lib/search";

const toDay = (ms: number | null) => (ms === null ? "" : new Date(ms).toISOString().slice(0, 10));
const fromDay = (text: string, endOfDay: boolean) => (text ? Date.parse(`${text}T${endOfDay ? "23:59:59" : "00:00:00"}`) : null);

export function Filters({ value, onChange, tags }: { value: Filter; onChange: (f: Filter) => void; tags: string[] }) {
  const set = (patch: Partial<Filter>) => onChange({ ...value, ...patch });
  return <div className="filters">
    <input placeholder={t("搜索标题或备注")} value={value.text} onChange={(e) => set({ text: e.target.value })} />
    <label><input type="checkbox" checked={value.inBody} onChange={(e) => set({ inBody: e.target.checked })} /> {t("搜索正文（仅已读取的对话）")}</label>
    <select value={value.tag} onChange={(e) => set({ tag: e.target.value })}>
      <option value="">{t("全部标签")}</option>
      {tags.map((tag) => <option key={tag} value={tag}>{tag}</option>)}
    </select>
    <select value={value.body} onChange={(e) => set({ body: e.target.value as Filter["body"] })}>
      <option value="any">{t("正文：全部")}</option>
      <option value="read">{t("正文已读")}</option>
      <option value="unread">{t("正文未读")}</option>
    </select>
    <input type="date" value={toDay(value.from)} onChange={(e) => set({ from: fromDay(e.target.value, false) })} aria-label={t("起始日期")} />
    <input type="date" value={toDay(value.to)} onChange={(e) => set({ to: fromDay(e.target.value, true) })} aria-label={t("结束日期")} />
    <label><input type="checkbox" checked={value.favorite} onChange={(e) => set({ favorite: e.target.checked })} /> {t("仅收藏")}</label>
    <label><input type="checkbox" checked={value.showRemoved} onChange={(e) => set({ showRemoved: e.target.checked })} /> {t("显示已删除")}</label>
  </div>;
}
```

`extension/src/ui/manage/Detail.tsx`：

```tsx
import { useEffect, useState } from "react";
import { t } from "../../i18n";
import { addTag, getBody, listExcerpts, updateLocal, type Conversation, type Db, type Excerpt, type Folder, type StoredBody } from "../../lib/db";
import { conversationUrl } from "../../sites/registry";

export function Detail({ db, conv, folders, onRead, onChanged, reading }: {
  db: Db; conv: Conversation; folders: Folder[]; reading: boolean;
  onRead: (c: Conversation) => void; onChanged: () => void;
}) {
  const [body, setBody] = useState<StoredBody | undefined>();
  const [excerpts, setExcerpts] = useState<Excerpt[]>([]);
  const [note, setNote] = useState(conv.note);
  const [tag, setTag] = useState("");
  useEffect(() => {
    setNote(conv.note);
    void getBody(db, conv.key).then(setBody);
    void listExcerpts(db, conv.site, conv.id).then(setExcerpts);
  }, [db, conv]);
  const url = conversationUrl(conv.site, conv.id);
  const save = async (patch: Parameters<typeof updateLocal>[2]) => { await updateLocal(db, [conv.key], patch); onChanged(); };

  return <div>
    <h3>{conv.title || t("（无标题）")}</h3>
    <p><a href={url} target="_blank" rel="noreferrer">{t("打开原对话")}</a></p>
    <div className="row">
      <label><input type="checkbox" checked={conv.favorite} onChange={(e) => void save({ favorite: e.target.checked })} /> {t("收藏")}</label>
      <select value={conv.folderId ?? ""} onChange={(e) => void save({ folderId: e.target.value || null })}>
        <option value="">{t("未归入文件夹")}</option>
        {folders.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
      </select>
    </div>
    <div className="row" style={{ marginTop: 6 }}>
      {conv.tags.map((x) => <span key={x} className="chip">{x} <button aria-label={t("移除标签")} onClick={() => void save({ tags: conv.tags.filter((y) => y !== x) })}>×</button></span>)}
      <input placeholder={t("加标签")} value={tag} onChange={(e) => setTag(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") { void addTag(db, [conv.key], tag).then(onChanged); setTag(""); } }} />
    </div>
    <textarea style={{ width: "100%", marginTop: 6 }} rows={3} placeholder={t("备注")} value={note} onChange={(e) => setNote(e.target.value)} onBlur={() => { if (note !== conv.note) void save({ note }); }} />
    <div className="row"><button disabled={reading || conv.removedAt !== null} onClick={() => onRead(conv)}>{t(body ? "重新读取正文" : "读取正文")}</button>
      {body && <span className="mut">{t("读取于")} {new Date(conv.bodyFetchedAt ?? 0).toLocaleString()}</span>}</div>
    {excerpts.length > 0 && <><h4>{t("摘录")}</h4>{excerpts.map((x) => <div key={x.id} className="msg">{x.text}{x.note && <div className="mut">{x.note}</div>}</div>)}</>}
    {body?.messages.map((m, i) => <div key={i} className="msg"><b>{m.role === "user" ? t("用户") : m.role === "assistant" ? t("助手") : m.role}</b>{"\n"}{m.text}</div>)}
  </div>;
}
```

`extension/src/ui/manage/App.tsx`：

```tsx
import { useCallback, useEffect, useMemo, useState } from "react";
import { t } from "../../i18n";
import {
  addTag, bodyIsFresh, createFolder, deleteFolder, getBody, listAccounts, listConversations, listFolders,
  openDb, putBody, renameAccount, renameFolder, updateLocal, type Account, type Conversation, type Db, type Folder,
} from "../../lib/db";
import { runDeleteJob } from "../../lib/deleteJob";
import { saveFile } from "../../lib/download";
import { exportFileName, toMarkdown, type ExportMode } from "../../lib/markdown";
import { createPacer } from "../../lib/pacer";
import { refreshIndex } from "../../lib/refresh";
import { allTags, applyFilter, bodyText, EMPTY_FILTER, type Filter } from "../../lib/search";
import { createSiteApi } from "../../lib/siteClient";
import type { SiteId } from "../../shared/types";
import { conversationUrl, SITES } from "../../sites/registry";
import { errorText } from "../errors";
import { ConversationList } from "./ConversationList";
import { DeleteDialog } from "./DeleteDialog";
import { Detail } from "./Detail";
import { Filters } from "./Filters";

const api = createSiteApi();
const urlOf = (c: Conversation) => conversationUrl(c.site, c.id);

export function App() {
  const [db, setDb] = useState<Db | null>(null);
  const [convs, setConvs] = useState<Conversation[]>([]);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [bodies, setBodies] = useState(new Map<string, string>());
  const [filter, setFilter] = useState<Filter>(EMPTY_FILTER);
  const [selected, setSelected] = useState(new Set<string>());
  const [active, setActive] = useState<Conversation | null>(null);
  const [busy, setBusy] = useState("");
  const [message, setMessage] = useState("");
  const [deleting, setDeleting] = useState<Conversation[] | null>(null);
  const [current, setCurrent] = useState(new Set<string>());

  const reload = useCallback(async (d: Db) => {
    const [c, a, f] = await Promise.all([listConversations(d), listAccounts(d), listFolders(d)]);
    setConvs(c); setAccounts(a); setFolders(f);
    setActive((old) => (old ? c.find((x) => x.key === old.key) ?? null : null));
  }, []);
  useEffect(() => { void openDb().then(async (d) => { setDb(d); await reload(d); }); }, [reload]);

  useEffect(() => {
    if (!db || !filter.inBody) return;
    void (async () => {
      const map = new Map<string, string>();
      for (const c of convs) if (c.bodyFetchedAt !== null) { const b = await getBody(db, c.key); if (b) map.set(c.key, bodyText(b)); }
      setBodies(map);
    })();
  }, [db, convs, filter.inBody]);

  const shown = useMemo(() => applyFilter(convs, filter, bodies), [convs, filter, bodies]);
  const aliasOf = useCallback((key: string) => accounts.find((a) => a.key === key)?.alias ?? key, [accounts]);
  const chosen = convs.filter((c) => selected.has(c.key));

  async function guarded(label: string, task: () => Promise<void>) {
    setBusy(label); setMessage("");
    try { await task(); } catch (e) { setMessage(errorText(e)); } finally { setBusy(""); if (db) await reload(db); }
  }

  const refresh = (site: SiteId) => guarded(t("正在刷新列表"), async () => {
    const r = await refreshIndex(api, db!, site, createPacer(), (n) => setBusy(`${t("正在刷新列表")} ${n}`));
    setCurrent((old) => new Set([...old, r.account.key]));
    setMessage(`${SITES[site].label}：${t("共")} ${r.total}，${t("新增")} ${r.added}，${t("已删除")} ${r.removed}`);
  });

  const readBody = (c: Conversation) => guarded(t("正在读取正文"), async () => {
    await putBody(db!, c.key, await api.read(c.site, c.id), Date.now());
  });

  const exportChosen = (mode: ExportMode) => guarded(t("正在导出"), async () => {
    const pacer = createPacer();
    for (const c of chosen) {
      let body = bodyIsFresh(c) ? await getBody(db!, c.key) : undefined;
      if (!body) { await pacer.wait(); const fresh = await api.read(c.site, c.id); await putBody(db!, c.key, fresh, Date.now()); body = { ...fresh, key: c.key }; }
      await saveFile(exportFileName(c, "md"), toMarkdown(c, aliasOf(c.account), body, mode, urlOf(c)), "text/markdown");
    }
    setMessage(`${t("已导出")} ${chosen.length} ${t("条到下载目录的「Stacker 网页对话」文件夹")}`);
  });

  async function openDelete() {
    const sites = [...new Set(chosen.map((c) => c.site))];
    const signedIn = new Set<string>();
    for (const site of sites) {
      try { const a = await api.account(site); signedIn.add(`${site}:${a.remoteId}`); } catch { /* shown as other account */ }
    }
    setCurrent(signedIn);
    setDeleting(chosen);
  }

  if (!db) return <p style={{ padding: 16 }}>{t("正在打开…")}</p>;
  const siteAccounts = accounts.filter((a) => !filter.site || a.site === filter.site);
  return <div className="layout">
    <div className="top">
      <b>{t("Stacker 网页对话")}</b>
      <select value={filter.site} onChange={(e) => setFilter({ ...filter, site: e.target.value as SiteId | "", account: "" })}>
        <option value="">{t("全部站点")}</option>
        {(Object.keys(SITES) as SiteId[]).map((s) => <option key={s} value={s}>{SITES[s].label}</option>)}
      </select>
      <select value={filter.account} onChange={(e) => setFilter({ ...filter, account: e.target.value })}>
        <option value="">{t("全部账号")}</option>
        {siteAccounts.map((a) => <option key={a.key} value={a.key}>{a.alias}</option>)}
      </select>
      {filter.account && <button onClick={() => { const alias = prompt(t("账号备注名"), aliasOf(filter.account)); if (alias) void renameAccount(db, filter.account, alias).then(() => reload(db)); }}>{t("改备注名")}</button>}
      {(Object.keys(SITES) as SiteId[]).map((s) => <button key={s} disabled={!!busy} onClick={() => void refresh(s)}>{t("刷新")} {SITES[s].label}</button>)}
      {busy && <span className="mut">{busy}…</span>}
      {message && <span className={message.includes("：") ? "mut" : "err"}>{message}</span>}
    </div>
    <div className="side">
      <div><button onClick={() => setFilter({ ...filter, folder: "" })}>{t("全部")}</button></div>
      <div><button onClick={() => setFilter({ ...filter, folder: "none" })}>{t("未归入")}</button></div>
      {folders.map((f) => <div key={f.id} className="row">
        <button onClick={() => setFilter({ ...filter, folder: f.id })}>{f.name}</button>
        <button aria-label={t("重命名")} onClick={() => { const name = prompt(t("文件夹名称"), f.name); if (name) void renameFolder(db, f.id, name).then(() => reload(db)); }}>✎</button>
        <button aria-label={t("删除文件夹")} onClick={() => { if (confirm(t("删除文件夹？其中的对话不会被删除。"))) void deleteFolder(db, f.id).then(() => reload(db)); }}>×</button>
      </div>)}
      <button onClick={() => { const name = prompt(t("文件夹名称")); if (name) void createFolder(db, name, Date.now()).then(() => reload(db)); }}>＋ {t("新建文件夹")}</button>
    </div>
    <div className="main">
      <Filters value={filter} onChange={setFilter} tags={allTags(convs)} />
      {selected.size > 0 && <div className="row" style={{ marginBottom: 6 }}>
        <select defaultValue="" onChange={(e) => { if (e.target.value) void updateLocal(db, [...selected], { folderId: e.target.value === "none" ? null : e.target.value }).then(() => reload(db)); e.target.value = ""; }}>
          <option value="">{t("移到文件夹…")}</option>
          <option value="none">{t("未归入")}</option>
          {folders.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
        </select>
        <button onClick={() => { const tag = prompt(t("标签")); if (tag) void addTag(db, [...selected], tag).then(() => reload(db)); }}>{t("加标签")}</button>
        <button onClick={() => void updateLocal(db, [...selected], { favorite: true }).then(() => reload(db))}>{t("收藏")}</button>
        <button disabled={!!busy} onClick={() => void exportChosen("slim")}>{t("导出精简版")}</button>
        <button disabled={!!busy} onClick={() => void exportChosen("full")}>{t("导出完整版")}</button>
        <button className="danger" disabled={!!busy} onClick={() => void openDelete()}>{t("删除…")}</button>
      </div>}
      {shown.length === 0 ? <p className="mut">{t("没有对话。先打开并登录 ChatGPT 或 Claude，再点「刷新」。")}</p>
        : <ConversationList items={shown} aliasOf={aliasOf} selected={selected} active={active?.key ?? null} onOpen={setActive}
          onSelect={(keys, on) => setSelected((old) => { const next = new Set(old); keys.forEach((k) => (on ? next.add(k) : next.delete(k))); return next; })} />}
    </div>
    <div className="detail">
      {active ? <Detail db={db} conv={active} folders={folders} reading={!!busy} onRead={(c) => void readBody(c)} onChanged={() => void reload(db)} />
        : <p className="mut">{t("选择一条对话查看详情")}</p>}
    </div>
    {deleting && <DeleteDialog items={deleting} currentAccounts={current}
      onRun={(mode, signal, onProgress) => runDeleteJob(deleting, mode, { api, db, pacer: createPacer(), now: Date.now, save: saveFile, aliasOf }, signal, onProgress)}
      onClose={(changed) => { setDeleting(null); if (changed) { setSelected(new Set()); void reload(db); } }} />}
  </div>;
}
```

`extension/src/ui/manage/main.tsx`：

```tsx
import { createRoot } from "react-dom/client";
import "../style.css";
import { App } from "./App";

createRoot(document.getElementById("root")!).render(<App />);
```

- [ ] **Step 5: 补英文对照**

把本任务出现的全部 `t("…")` 字面量、`ERROR_TEXT` 的值、`MODES` 的 `label` 与 `hint` 加入 `extension/src/i18n.ts` 的 `EN`。运行 `npx vitest run extension/src/i18n.test.ts`，把报告缺失的每一项补齐，直到通过。

- [ ] **Step 6: 运行测试与检查**

Run: `npx vitest run extension/src && npm run typecheck && npm run lint && npm run ext:build`
Expected: 全部 PASS，构建成功。

- [ ] **Step 7: Commit**

```bash
git add extension/src
git commit -m "feat(extension): manage page for browsing, organizing, exporting and deleting chats"
```

---

### Task 12: 后台脚本与对话小窗

**Files:**
- Create: `extension/src/lib/lastTab.ts`, `extension/src/lib/lastTab.test.ts`, `extension/src/ui/popup/Popup.tsx`
- Modify: `extension/src/background.ts`, `extension/src/ui/popup/main.tsx`, `extension/src/i18n.ts`

**Interfaces:**
- Consumes: registry（Task 4），db（Task 6），`errorText`（Task 11）。
- Produces:

```ts
// lastTab.ts
export interface SiteTab { tabId: number; site: SiteId; url: string; title: string }
export function siteTabOf(tab: { id?: number; url?: string; title?: string }): SiteTab | null;
export const LAST_TAB_KEY = "lastSiteTab";
```

后台行为：
- 点插件图标：打开管理页（已打开则切过去）。
- 快捷键 `open-popup`：打开（或聚焦）小窗 `chrome.windows.create({ url: "popup.html", type: "popup", width: 460, height: 720 })`，窗口 ID 存 `chrome.storage.session`。
- `tabs.onActivated` / `tabs.onUpdated`：当前标签页是站点对话时写入 `chrome.storage.session[LAST_TAB_KEY]`。
- 消息 `{ type: "save-excerpt", ... }`：写入 IndexedDB（Task 13 使用）。

小窗：读取 `LAST_TAB_KEY` 并监听变化，显示当前对话（标题、站点）；对应的 `Conversation` 若已在索引中，可收藏、改文件夹、加标签、写备注；列出该对话的摘录（可删除）；按钮「在管理页打开」。未在索引中则提示「先在管理页刷新该站点」。

- [ ] **Step 1: 写失败的测试**

`extension/src/lib/lastTab.test.ts`：

```ts
import { describe, expect, it } from "vitest";
import { siteTabOf } from "./lastTab";

describe("siteTabOf", () => {
  it("keeps only conversation pages of supported sites", () => {
    expect(siteTabOf({ id: 3, url: "https://claude.ai/chat/k1", title: "Refactor - Claude" })).toEqual({ tabId: 3, site: "claude", url: "https://claude.ai/chat/k1", title: "Refactor - Claude" });
    expect(siteTabOf({ id: 3, url: "https://claude.ai/new" })).toBeNull();
    expect(siteTabOf({ id: 3, url: "https://example.com/c/x" })).toBeNull();
    expect(siteTabOf({ url: "https://chatgpt.com/c/x" })).toBeNull();
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/lib/lastTab.test.ts`
Expected: FAIL

- [ ] **Step 3: 实现**

`extension/src/lib/lastTab.ts`：

```ts
import type { SiteId } from "../shared/types";
import { conversationIdOfUrl, siteOfUrl } from "../sites/registry";

export interface SiteTab { tabId: number; site: SiteId; url: string; title: string }
export const LAST_TAB_KEY = "lastSiteTab";

export function siteTabOf(tab: { id?: number; url?: string; title?: string }): SiteTab | null {
  if (typeof tab.id !== "number" || !tab.url) return null;
  const site = siteOfUrl(tab.url);
  if (!site || !conversationIdOfUrl(site, tab.url)) return null;
  return { tabId: tab.id, site, url: tab.url, title: tab.title ?? "" };
}
```

`extension/src/background.ts`：

```ts
import { addExcerpt, openDb } from "./lib/db";
import { LAST_TAB_KEY, siteTabOf } from "./lib/lastTab";
import type { SiteId } from "./shared/types";

const MANAGE = chrome.runtime.getURL("manage.html");

chrome.action.onClicked.addListener(async () => {
  const [tab] = await chrome.tabs.query({ url: MANAGE });
  if (tab?.id) { await chrome.tabs.update(tab.id, { active: true }); if (tab.windowId) await chrome.windows.update(tab.windowId, { focused: true }); }
  else await chrome.tabs.create({ url: MANAGE });
});

chrome.commands.onCommand.addListener(async (command) => {
  if (command !== "open-popup") return;
  const { popupWindow } = await chrome.storage.session.get("popupWindow");
  if (typeof popupWindow === "number") {
    try { await chrome.windows.update(popupWindow, { focused: true }); return; } catch { /* closed */ }
  }
  const win = await chrome.windows.create({ url: chrome.runtime.getURL("popup.html"), type: "popup", width: 460, height: 720 });
  await chrome.storage.session.set({ popupWindow: win?.id });
});

async function remember(tab: chrome.tabs.Tab) {
  const siteTab = siteTabOf(tab);
  if (siteTab) await chrome.storage.session.set({ [LAST_TAB_KEY]: siteTab });
}
chrome.tabs.onActivated.addListener(({ tabId }) => { void chrome.tabs.get(tabId).then(remember).catch(() => {}); });
chrome.tabs.onUpdated.addListener((_id, change, tab) => { if (change.url || change.title) void remember(tab); });

export interface SaveExcerpt { type: "save-excerpt"; site: SiteId; conversationId: string | null; url: string; pageTitle: string; text: string }

chrome.runtime.onMessage.addListener((message: unknown, _sender, reply) => {
  const m = message as SaveExcerpt;
  if (m?.type !== "save-excerpt") return false;
  void openDb()
    .then((db) => addExcerpt(db, { site: m.site, conversationId: m.conversationId, url: m.url, pageTitle: m.pageTitle, text: m.text.slice(0, 20_000), note: "" }, Date.now()))
    .then(() => reply({ ok: true }), (e) => reply({ ok: false, error: String(e) }));
  return true;
});
```

`extension/src/ui/popup/Popup.tsx`：

```tsx
import { useCallback, useEffect, useState } from "react";
import { t } from "../../i18n";
import { addTag, conversationKey, deleteExcerpt, getConversation, listExcerpts, listFolders, openDb, updateLocal, type Conversation, type Db, type Excerpt, type Folder } from "../../lib/db";
import { LAST_TAB_KEY, type SiteTab } from "../../lib/lastTab";
import { conversationIdOfUrl, SITES } from "../../sites/registry";

export function Popup() {
  const [db, setDb] = useState<Db | null>(null);
  const [tab, setTab] = useState<SiteTab | null>(null);
  const [conv, setConv] = useState<Conversation | null>(null);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [excerpts, setExcerpts] = useState<Excerpt[]>([]);
  const [tag, setTag] = useState("");

  useEffect(() => {
    void openDb().then(setDb);
    void chrome.storage.session.get(LAST_TAB_KEY).then((v) => setTab((v[LAST_TAB_KEY] as SiteTab) ?? null));
    const onChange = (changes: Record<string, chrome.storage.StorageChange>) => { if (changes[LAST_TAB_KEY]) setTab(changes[LAST_TAB_KEY].newValue as SiteTab); };
    chrome.storage.session.onChanged.addListener(onChange);
    return () => chrome.storage.session.onChanged.removeListener(onChange);
  }, []);

  const id = tab ? conversationIdOfUrl(tab.site, tab.url) : null;
  const load = useCallback(async () => {
    if (!db || !tab || !id) return;
    setConv((await getConversation(db, conversationKey(tab.site, id))) ?? null);
    setFolders(await listFolders(db));
    setExcerpts(await listExcerpts(db, tab.site, id));
  }, [db, tab, id]);
  useEffect(() => { void load(); }, [load]);

  if (!tab || !id) return <p style={{ padding: 12 }}>{t("在 ChatGPT 或 Claude 打开一条对话后，这里会显示它。")}</p>;
  const save = async (patch: Parameters<typeof updateLocal>[2]) => { if (db && conv) { await updateLocal(db, [conv.key], patch); await load(); } };
  return <div style={{ padding: 12 }}>
    <div className="mut">{SITES[tab.site].label}</div>
    <h3>{conv?.title || tab.title}</h3>
    {!conv && <p className="warn">{t("这条对话还不在列表里：请在管理页刷新该站点。")}</p>}
    {conv && db && <>
      <div className="row">
        <label><input type="checkbox" checked={conv.favorite} onChange={(e) => void save({ favorite: e.target.checked })} /> {t("收藏")}</label>
        <select value={conv.folderId ?? ""} onChange={(e) => void save({ folderId: e.target.value || null })}>
          <option value="">{t("未归入文件夹")}</option>
          {folders.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
        </select>
      </div>
      <div className="row" style={{ marginTop: 6 }}>
        {conv.tags.map((x) => <span key={x} className="chip">{x}</span>)}
        <input placeholder={t("加标签")} value={tag} onChange={(e) => setTag(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") { void addTag(db, [conv.key], tag).then(load); setTag(""); } }} />
      </div>
      <textarea style={{ width: "100%", marginTop: 6 }} rows={4} placeholder={t("备注")} defaultValue={conv.note} onBlur={(e) => void save({ note: e.target.value })} />
    </>}
    <h4>{t("摘录")}（{excerpts.length}）</h4>
    <p className="mut">{t("在网页上选中文字，点出现的「存为摘录」按钮即可添加。")}</p>
    {excerpts.map((x) => <div key={x.id} className="msg">{x.text}
      <div><button onClick={() => { if (db) void deleteExcerpt(db, x.id).then(load); }}>{t("删除")}</button></div></div>)}
    <button onClick={() => void chrome.tabs.create({ url: chrome.runtime.getURL("manage.html") })}>{t("在管理页打开")}</button>
  </div>;
}
```

`extension/src/ui/popup/main.tsx`：

```tsx
import { createRoot } from "react-dom/client";
import "../style.css";
import { Popup } from "./Popup";

createRoot(document.getElementById("root")!).render(<Popup />);
```

补齐 `EN` 中新增文案（运行 i18n 测试核对）。

- [ ] **Step 4: 运行测试与检查**

Run: `npx vitest run extension/src && npm run typecheck && npm run lint && npm run ext:build`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add extension/src
git commit -m "feat(extension): popup window for the conversation on screen"
```

---

### Task 13: 网页上的摘录按钮

**Files:**
- Create: `extension/src/content/excerpt.ts`, `extension/src/content/excerpt.test.ts`
- Modify: `extension/src/content/index.ts`

**Interfaces:**
- Consumes: `siteOfUrl`、`conversationIdOfUrl`（Task 4），`SaveExcerpt` 消息（Task 12）。
- Produces: `export function excerptPayload(href: string, title: string, selected: string): SaveExcerpt | null`；`export function installExcerptButton(doc?: Document): void`。

按钮：选区在页面内且非空（去空白后 ≥ 2 字）时，在选区末尾附近显示一个固定定位的小按钮「存为摘录」，放在 Shadow DOM 里，避免被网站样式影响、也不改网站 DOM 结构；点击发送 `save-excerpt`，成功显示「已存」1.5 秒后消失；选区清空或滚动时隐藏。

- [ ] **Step 1: 写失败的测试**

`extension/src/content/excerpt.test.ts`：

```ts
// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { excerptPayload } from "./excerpt";

describe("excerptPayload", () => {
  it("builds a save message from the page and the selection", () => {
    expect(excerptPayload("https://chatgpt.com/c/abc", "Trip - ChatGPT", "  Kyoto in autumn  ")).toEqual({
      type: "save-excerpt", site: "chatgpt", conversationId: "abc", url: "https://chatgpt.com/c/abc", pageTitle: "Trip - ChatGPT", text: "Kyoto in autumn",
    });
    expect(excerptPayload("https://claude.ai/new", "", "some text")?.conversationId).toBeNull();
    expect(excerptPayload("https://claude.ai/chat/k", "", " a ")).toBeNull();
    expect(excerptPayload("https://example.com/", "", "hello")).toBeNull();
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/content/excerpt.test.ts`
Expected: FAIL

- [ ] **Step 3: 实现**

`extension/src/content/excerpt.ts`：

```ts
import type { SaveExcerpt } from "../background";
import { t } from "../i18n";
import { conversationIdOfUrl, siteOfUrl } from "../sites/registry";

export function excerptPayload(href: string, title: string, selected: string): SaveExcerpt | null {
  const site = siteOfUrl(href);
  const text = selected.trim();
  if (!site || text.length < 2) return null;
  return { type: "save-excerpt", site, conversationId: conversationIdOfUrl(site, href), url: href, pageTitle: title, text };
}

/** A single floating button in its own shadow root; the site's page structure is left alone. */
export function installExcerptButton(doc: Document = document): void {
  const host = doc.createElement("stacker-excerpt");
  const root = host.attachShadow({ mode: "closed" });
  const button = doc.createElement("button");
  button.textContent = t("存为摘录");
  button.setAttribute("style", "position:fixed;z-index:2147483647;display:none;padding:4px 10px;border-radius:6px;border:1px solid #f5821f;background:#f5821f;color:#fff;font:12px system-ui;cursor:pointer;box-shadow:0 2px 8px rgba(0,0,0,.25)");
  root.append(button);
  doc.documentElement.append(host);

  let pending: SaveExcerpt | null = null;
  const hide = () => { button.style.display = "none"; pending = null; };

  doc.addEventListener("mouseup", () => {
    setTimeout(() => {
      const selection = doc.getSelection();
      const payload = selection && !selection.isCollapsed ? excerptPayload(location.href, doc.title, selection.toString()) : null;
      if (!payload || !selection?.rangeCount) return hide();
      const rect = selection.getRangeAt(0).getBoundingClientRect();
      pending = payload;
      button.textContent = t("存为摘录");
      button.style.left = `${Math.min(rect.right + 6, innerWidth - 110)}px`;
      button.style.top = `${Math.max(rect.bottom + 6, 6)}px`;
      button.style.display = "block";
    }, 0);
  });
  doc.addEventListener("scroll", hide, true);
  button.addEventListener("mousedown", (e) => e.preventDefault());
  button.addEventListener("click", async () => {
    if (!pending) return;
    const res = (await chrome.runtime.sendMessage(pending)) as { ok: boolean } | undefined;
    button.textContent = res?.ok ? t("已存") : t("保存失败");
    pending = null;
    setTimeout(hide, 1500);
  });
}
```

`extension/src/content/index.ts`：

```ts
import { installAgent } from "./agent";
import { installExcerptButton } from "./excerpt";

installAgent();
installExcerptButton();
```

注意：`import type { SaveExcerpt } from "../background"` 只引类型，不会把后台脚本打进内容脚本。补齐 `EN`：「存为摘录」「已存」「保存失败」。

- [ ] **Step 4: 运行测试与构建**

Run: `npx vitest run extension/src && npm run typecheck && npm run lint && npm run ext:build`
Expected: PASS；`extension/dist/content.js` 为 IIFE，不含 `import` 语句（`grep -c "^import" extension/dist/content.js` 输出 0）。

- [ ] **Step 5: Commit**

```bash
git add extension/src/content extension/src/i18n.ts
git commit -m "feat(extension): save a selection as an excerpt"
```

---

### Task 14: 打包、说明与文档

**Files:**
- Create: `extension/scripts/zip.mjs`, `extension/README.md`
- Modify: `docs/superpowers/specs/2026-09-19-web-chat-extension-design.md`（状态改为「G1 已实现」）, `README.zh-CN.md`, `README.md`（各加一小节链接到 `extension/README.md`）

**Interfaces:**
- Consumes: `npm run ext:build`。
- Produces: `npm run ext:zip` → `extension/stacker-web-chats-<version>.zip`。

- [ ] **Step 1: 打包脚本**

`extension/scripts/zip.mjs`（用 PowerShell 自带的 `Compress-Archive`，不新增依赖）：

```js
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const { version } = JSON.parse(readFileSync(join(root, "dist", "manifest.json"), "utf8"));
const out = join(root, `stacker-web-chats-${version}.zip`);
execFileSync("powershell.exe", ["-NoProfile", "-Command", `Compress-Archive -Path '${join(root, "dist", "*")}' -DestinationPath '${out}' -Force`], { stdio: "inherit" });
console.log(out);
```

- [ ] **Step 2: 使用说明**

`extension/README.md`（中文），包含：
- 用途与原则（数据只在本机，不保存凭证，不上报）。
- 安装：`npm run ext:build`，Chrome 打开 `chrome://extensions`（Edge 为 `edge://extensions`），打开「开发者模式」，「加载已解压的扩展程序」选 `extension/dist`；插件 ID 固定为 `EXTENSION_ID` 中的值；Chrome 提示停用开发者模式扩展时点「保留」。
- 使用：先打开并登录 chatgpt.com / claude.ai（已打开的页面在安装后刷新一次），点插件图标打开管理页，「刷新」读取标题；正文按需读取；`Alt+Shift+S` 打开对话小窗；选中文字存为摘录。
- 删除：三种模式、限速、可中止；只删除当前登录账号的对话；导出位置为下载目录下 `Stacker 网页对话/`。
- 站点接口变化时的表现（停止并提示）。

- [ ] **Step 3: 构建与打包**

Run: `npm run ext:zip`
Expected: 输出 zip 路径，zip 内有 `manifest.json`。

- [ ] **Step 4: Commit**

```bash
git add extension/scripts/zip.mjs extension/README.md README.md README.zh-CN.md docs/superpowers/specs/2026-09-19-web-chat-extension-design.md
git commit -m "docs(extension): install and usage guide, zip packaging"
```

---

### Task 15: 实机核对与验收（需要用户参与）

本任务由协调者与用户一起完成，不交给子代理。

- [ ] **Step 1: 只读核对接口（征得用户同意后）**

在用户的 Chrome（已登录 ChatGPT、Claude）中，用 Claude in Chrome 在各站点页面执行只读请求，核对 Task 3、Task 4 假定的返回形状：
- ChatGPT：`/api/auth/session`（只看字段名，不输出 token 与邮箱）、`/backend-api/conversations?offset=0&limit=3&order=updated`、`/backend-api/conversation/<一条对话>`。
- Claude：`/api/organizations`、`/api/organizations/<org>/chat_conversations?limit=3&offset=0`、单条对话详情。

只记录字段名与类型，不记录对话内容。形状与样本不同时：更新 `fixtures/`、调整解析与测试（对外接口不变），提交 `fix(extension): match <site> responses`。

- [ ] **Step 2: 用户加载插件后的只读验收**

用户按 `extension/README.md` 加载插件。核对：刷新两个站点的列表、条数与网页一致；读取几条正文，分支对话只显示当前分支；搜索、文件夹、标签、收藏、备注、导出精简版与完整版；小窗与摘录按钮。

- [ ] **Step 3: 删除验收（仅针对用户专门新建的测试对话，事先征得同意）**

用户在每个站点新建 2–3 条测试对话；刷新后只选中这些测试对话，分别用三种模式删除（直接删除要经过二次确认）；核对导出文件、网站上已消失、列表标为已删除；中途点「中止」能停下。

- [ ] **Step 4: 汇报**

向用户汇报核对结果、任何接口差异及修复。

---

## Self-Review

- **Spec 覆盖（G1 范围）**：ChatGPT、Claude 适配器（Task 3、4）；按需读正文（Task 11 读取、导出与删除前读取）；搜索（Task 8）；文件夹与标签、收藏、备注（Task 6、11、12）；导出 Markdown 精简 / 完整（Task 9、11）；三种删除模式、二次确认、限速、退避、中止、账号不符跳过、适配失效停止（Task 7、10、11）；多账号隔离与备注名（Task 6、11）；弹出小窗（Task 12）；摘录按钮（Task 13）；固定插件 ID（Task 1）；只申请两个站点权限、不存凭证与邮箱（Task 1、3）；开发者模式安装与 zip（Task 14）；实机只读验证、删除只测测试对话（Task 15）。G2–G4（其他站点、桥接与同步、提炼）不在本计划。
- **占位扫描**：各步骤均给出代码或确切命令；i18n 英文对照由测试驱动逐项补齐（Task 11、12、13 已写明）。
- **类型一致**：`SiteApi`、`Adapter`、`Conversation`、`StoredBody`、`Pacer`、`DeleteMode`、`ItemResult`、`SaveExcerpt`、`SiteTab` 的名称与签名在定义任务与使用任务中一致；对话主键统一 `${site}:${id}`，账号主键 `${site}:${remoteId}`。
