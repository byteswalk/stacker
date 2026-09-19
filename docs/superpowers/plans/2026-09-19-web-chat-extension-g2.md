# G2：Gemini、Grok、DeepSeek 站点适配器 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 G1 插件上增加 Gemini、Grok、DeepSeek 三个站点：标题索引、按需读正文、导出；未经实机核对的站点在管理页标「未实测」并禁止删除。

**Architecture:** 三个适配器沿用 G1 模式（`sites/*.ts`，纯函数 + 注入的 `fetchJson`，`guards` 校验结构，不符即 `E_BROKEN`），在各站点页面的内容脚本里运行。Gemini 走 Google 的 batchexecute 协议（表单请求、非 JSON 回复），其请求构造与回复解析放在单独的 `sites/batchexecute.ts`；页面令牌从同源 `/app` HTML 用正则提取（内容脚本读不到页面全局变量）。Grok、DeepSeek 需要读页面的 Cookie / localStorage，经新增的窄接口 `PageAccess` 注入，保持适配器可测。站点表每站增加 `verified` 标记，管理页据此显示「未实测」并通过删除对话框已有的「按站点报错」机制禁止删除。

**Tech Stack:** TypeScript 6、React 19、Vite 8、Vitest 3（node 与 jsdom 环境）、Chrome Manifest V3。

**Spec:** `docs/superpowers/specs/2026-09-19-web-chat-extension-design.md`（本计划只覆盖分期 G2）；G1 计划 `docs/superpowers/plans/2026-09-19-web-chat-extension-g1.md` 为结构与约定参照。

## Global Constraints

- 浏览器：Chromium 内核（Chrome、Edge），Manifest V3。
- 站点恰为 5 个：`https://chatgpt.com/*`、`https://claude.ai/*`、`https://gemini.google.com/*`、`https://grok.com/*`、`https://chat.deepseek.com/*`；`host_permissions` 与内容脚本 `matches` 都恰好是这 5 项，不申请全部网站。
- 插件不保存、不导出任何凭证：Gemini 页面令牌（`SNlM0e`、`cfb2h`、`FdrFJe`）与 DeepSeek 的 `userToken` 只在内容脚本内存中使用；不存账号邮箱，Gemini 绝不读取 `oPEP7c`（邮箱）。
- 内容脚本运行在隔离环境，读不到页面全局变量（如 `WIZ_global_data`）；Gemini 令牌从同源 `https://gemini.google.com/app` 的 HTML 用正则提取并在适配器生命期内缓存。
- 未实测站点 `verified: false`：可刷新、读取、搜索、整理、导出，禁止删除；管理页显示「未实测」。ChatGPT、Claude 为 `true`；Grok、DeepSeek 本期保持 `false`；Gemini 仅在 Task 10 实测删除通过后改为 `true`。
- 适配器每次调用校验返回结构；不符即抛 `E_BROKEN`，绝不在结构不明时删除。
- 对话主键 `${site}:${id}`，账号主键 `${site}:${remoteId}`；Gemini 对话 ID 保留 `c_` 前缀（网址里去掉）。
- 对话网址：Gemini `https://gemini.google.com/app/<不带 c_ 的十六进制 ID>`、Grok `https://grok.com/c/<id>`、DeepSeek `https://chat.deepseek.com/a/chat/s/<id>`。
- 界面文案中文，英文走 `extension/src/i18n.ts` 的 `EN` 对照表；每个新增 `t("…")` 字面量都要有英文（`i18n.test.ts` 会检查）。
- 每个任务结束时 `npm run typecheck`、`npm run lint`、`npx vitest run` 全绿。
- 提交信息以 `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` 结尾。
- 使用 Write/Edit 工具写含反斜杠的内容，不用 bash heredoc。
- 实机核对：绝不替用户登录；只读核对只记录字段名与类型，不记录令牌、邮箱、对话内容；删除只针对用户同意新建的测试对话。

## 文件结构

```
extension/
  README.md                         5 个站点、未实测说明（Task 9）
  public/manifest.json              5 个站点权限、版本 0.2.0（Task 7）
  src/
    manifest.test.ts                检查 5 个站点且与站点表一致（Task 7）
    i18n.ts                         新文案英文（Task 8）
    content/agent.ts                把 PageAccess 交给适配器（Task 1）
    sites/
      http.ts                       FetchInit.form、FetchResult.text（Task 1）
      page.ts                       PageAccess：读 Cookie、localStorage（Task 1）
      types.ts                      AdapterFactory 增加可选 page 参数（Task 1）
      registry.ts                   SiteInfo：verified、idOfPath、urlOf；新站点条目（Task 2、4、5、6）
      batchexecute.ts               Gemini 令牌提取、请求构造、回复解析（Task 3）
      gemini.ts                     Gemini 适配器（Task 4）
      grok.ts                       Grok 适配器（Task 5）
      deepseek.ts                   DeepSeek 适配器（Task 6）
      fixtures/gemini.ts  fixtures/grok.ts  fixtures/deepseek.ts
    ui/
      manage/siteStatus.ts          siteName、deleteBlockReason（Task 8）
      manage/App.tsx                未实测标记、删除拦截、5 站文案（Task 8）
      popup/Popup.tsx               5 站文案（Task 8）
docs/superpowers/specs/2026-09-19-web-chat-extension-design.md   状态行（Task 9）
README.md  README.zh-CN.md                                       插件小节提到 5 个站点（Task 9）
```

---

### Task 1: HTTP 表单请求与原文回复、页面读取

Gemini 需要发送 `application/x-www-form-urlencoded` 表单并读取非 JSON 的回复原文；Grok 需要读 Cookie，DeepSeek 需要读 localStorage。本任务只加通道，不改任何现有适配器的行为。

**Files:**
- Modify: `extension/src/sites/http.ts`, `extension/src/sites/http.test.ts`, `extension/src/sites/types.ts`, `extension/src/content/agent.ts`
- Create: `extension/src/sites/page.ts`, `extension/src/sites/page.test.ts`

**Interfaces:**
- Consumes: G1 的 `FetchInit`、`FetchResult`、`FetchJson`、`browserFetchJson`、`expectOk`、`Adapter`、`AdapterFactory`、`createAgent`、`installAgent`。
- Produces:

```ts
// sites/http.ts
export interface FetchInit {
  method?: "GET" | "POST" | "PATCH" | "DELETE";
  body?: unknown;                    // sent as JSON
  form?: Record<string, string>;     // sent url-encoded instead of body
  headers?: Record<string, string>;
}
export interface FetchResult { status: number; json: unknown; retryAfter: string | null; text?: string }

// sites/page.ts
export interface PageAccess { cookie(name: string): string | null; storage(key: string): string | null }
export const NO_PAGE: PageAccess;
export function pageOf(doc: { cookie: string }, getStorage: () => Pick<Storage, "getItem">): PageAccess;

// sites/types.ts
export type AdapterFactory = (fetchJson: FetchJson, page?: PageAccess) => Adapter;
```

- [ ] **Step 1: 写失败的测试**

`extension/src/sites/http.test.ts`：把文件开头的三行 import 替换为

```ts
import { afterEach, describe, expect, it, vi } from "vitest";
import { reviveError, serializeError, SiteError } from "../shared/types";
import { browserFetchJson, expectOk } from "./http";
```

并在文件末尾追加：

```ts
describe("browserFetchJson", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("sends a form url-encoded and keeps the raw text of a reply that is not JSON", async () => {
    const fetchMock = vi.fn(async () => new Response(")]}'\n[1]", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    const res = await browserFetchJson("https://gemini.google.com/x", { method: "POST", form: { "f.req": "[1]", at: "a b" }, headers: { "X-Same-Domain": "1" } });
    expect(res).toEqual({ status: 200, json: null, retryAfter: null, text: ")]}'\n[1]" });
    const [, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(init.body).toBe("f.req=%5B1%5D&at=a+b");
    expect(init.headers).toEqual({ "Content-Type": "application/x-www-form-urlencoded;charset=UTF-8", "X-Same-Domain": "1" });
    expect(init.credentials).toBe("include");
  });

  it("still sends a body as JSON and parses a JSON reply", async () => {
    const fetchMock = vi.fn(async () => new Response('{"ok":true}', { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    const res = await browserFetchJson("https://grok.com/x", { method: "POST", body: { a: 1 } });
    expect(res.json).toEqual({ ok: true });
    expect(res.text).toBe('{"ok":true}');
    const [, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(init.body).toBe('{"a":1}');
    expect(init.headers).toEqual({ "Content-Type": "application/json" });
  });
});
```

`extension/src/sites/page.test.ts`：

```ts
import { describe, expect, it } from "vitest";
import { NO_PAGE, pageOf } from "./page";

describe("page access", () => {
  it("reads one cookie by its exact name, decoded", () => {
    const page = pageOf({ cookie: "a=1; x-userid=u%2042; xx-userid=no" }, () => ({ getItem: () => null }));
    expect(page.cookie("x-userid")).toBe("u 42");
    expect(page.cookie("missing")).toBeNull();
  });

  it("reads localStorage and treats a blocked storage as empty", () => {
    const page = pageOf({ cookie: "" }, () => ({ getItem: (k: string) => (k === "userToken" ? '{"value":"t"}' : null) }));
    expect(page.storage("userToken")).toBe('{"value":"t"}');
    expect(page.storage("other")).toBeNull();
    const blocked = pageOf({ cookie: "" }, () => { throw new Error("SecurityError"); });
    expect(blocked.storage("userToken")).toBeNull();
  });

  it("has an empty stand-in for callers without a page", () => {
    expect(NO_PAGE.cookie("x")).toBeNull();
    expect(NO_PAGE.storage("x")).toBeNull();
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/sites/http.test.ts extension/src/sites/page.test.ts`
Expected: FAIL（`./page` 不存在；表单请求的 `body` 不是编码后的表单、`text` 缺失）。

- [ ] **Step 3: 实现**

`extension/src/sites/http.ts` 整个文件替换为：

```ts
import { SiteError } from "../shared/types";

export interface FetchInit {
  method?: "GET" | "POST" | "PATCH" | "DELETE";
  /** Sent as JSON. */
  body?: unknown;
  /** Sent url-encoded (application/x-www-form-urlencoded) instead of `body`. */
  form?: Record<string, string>;
  headers?: Record<string, string>;
}
/** `text` is the raw reply, for sites that do not answer in plain JSON; `json` is null when the text is not JSON. */
export interface FetchResult { status: number; json: unknown; retryAfter: string | null; text?: string }
export type FetchJson = (url: string, init?: FetchInit) => Promise<FetchResult>;

/** Same-origin request from the site's page, so the page's sign-in cookies apply. */
export const browserFetchJson: FetchJson = async (url, init = {}) => {
  const form = init.form ? new URLSearchParams(init.form).toString() : undefined;
  const json = form === undefined && init.body !== undefined ? JSON.stringify(init.body) : undefined;
  const contentType = form !== undefined ? "application/x-www-form-urlencoded;charset=UTF-8" : json !== undefined ? "application/json" : null;
  let res: Response;
  try {
    res = await fetch(url, {
      method: init.method ?? "GET",
      credentials: "include",
      headers: { ...(contentType ? { "Content-Type": contentType } : {}), ...init.headers },
      body: form ?? json,
    });
  } catch (e) {
    throw new SiteError("E_NET", e instanceof Error ? e.message : String(e));
  }
  const text = await res.text();
  let parsed: unknown = null;
  if (text) { try { parsed = JSON.parse(text); } catch { parsed = null; } }
  return { status: res.status, json: parsed, retryAfter: res.headers.get("Retry-After"), text };
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

`extension/src/sites/page.ts`：

```ts
/**
 * What an adapter may read from its site's page besides making requests: one cookie or one localStorage entry.
 * Never page globals (a content script cannot see them) and never anything written back.
 */
export interface PageAccess {
  cookie(name: string): string | null;
  storage(key: string): string | null;
}

export const NO_PAGE: PageAccess = { cookie: () => null, storage: () => null };

export function pageOf(doc: { cookie: string }, getStorage: () => Pick<Storage, "getItem">): PageAccess {
  return {
    cookie(name) {
      for (const part of doc.cookie.split(";")) {
        const eq = part.indexOf("=");
        if (eq < 0 || part.slice(0, eq).trim() !== name) continue;
        const raw = part.slice(eq + 1).trim();
        try { return decodeURIComponent(raw); } catch { return raw; }
      }
      return null;
    },
    storage(key) {
      try { return getStorage().getItem(key); } catch { return null; }
    },
  };
}
```

`extension/src/sites/types.ts` 整个文件替换为：

```ts
import type { ListPage, RemoteAccount, RemoteBody, SiteId } from "../shared/types";
import type { FetchJson } from "./http";
import type { PageAccess } from "./page";

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

/** `page` gives the adapter the few things it may read from the site's page; tests and sites that need none leave it out. */
export type AdapterFactory = (fetchJson: FetchJson, page?: PageAccess) => Adapter;
```

`extension/src/content/agent.ts`：import 区追加 `import { pageOf } from "../sites/page";`，并把 `installAgent` 里的

```ts
  const agent = createAgent(SITES[site].factory(browserFetchJson));
```

改为

```ts
  const agent = createAgent(SITES[site].factory(browserFetchJson, pageOf(document, () => localStorage)));
```

- [ ] **Step 4: 运行测试与检查**

Run: `npx vitest run extension/src && npm run typecheck && npm run lint`
Expected: 全部 PASS。

- [ ] **Step 5: Commit**

```bash
git add extension/src/sites/http.ts extension/src/sites/http.test.ts extension/src/sites/page.ts extension/src/sites/page.test.ts extension/src/sites/types.ts extension/src/content/agent.ts
git commit -m "feat(extension): form requests, raw replies and page access for site adapters"
```

---

### Task 2: 站点表：每站网址规则与 verified 标记

把站点表里分散的 `ID_PATTERN`、`URL_OF` 收进每个站点的条目，并增加 `verified`。后续每个适配器任务只需加一个条目。

**Files:**
- Modify: `extension/src/sites/registry.ts`, `extension/src/sites/registry.test.ts`

**Interfaces:**
- Consumes: `AdapterFactory`（Task 1）、`chatgpt`、`claude`。
- Produces（`sites/registry.ts`，对外函数签名不变）：

```ts
export interface SiteInfo {
  label: string;
  factory: AdapterFactory;
  origin: string;
  match: string;
  verified: boolean;
  idOfPath(path: string): string | null;
  urlOf(id: string): string;
}
export const SITES: Record<SiteId, SiteInfo>;
export function siteOfUrl(url: string): SiteId | null;
export function conversationIdOfUrl(site: SiteId, url: string): string | null;
export function conversationUrl(site: SiteId, id: string): string;
```

- [ ] **Step 1: 写失败的测试**

`extension/src/sites/registry.test.ts` 整个文件替换为：

```ts
import { describe, expect, it } from "vitest";
import type { SiteId } from "../shared/types";
import { conversationIdOfUrl, conversationUrl, siteOfUrl, SITES } from "./registry";

/** One conversation id per site, in the form its adapter uses. */
const SAMPLE_ID: Record<SiteId, string> = { chatgpt: "abc-1", claude: "k1" };

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

  it("finds every site's conversation id in the URL it builds for it", () => {
    for (const site of Object.keys(SITES) as SiteId[]) {
      const url = conversationUrl(site, SAMPLE_ID[site]);
      expect(siteOfUrl(url)).toBe(site);
      expect(conversationIdOfUrl(site, url)).toBe(SAMPLE_ID[site]);
    }
  });

  it("marks which sites were checked on a real account", () => {
    const verified = Object.fromEntries((Object.keys(SITES) as SiteId[]).map((s) => [s, SITES[s].verified]));
    expect(verified).toEqual({ chatgpt: true, claude: true });
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/sites/registry.test.ts`
Expected: FAIL（`verified` 为 `undefined`）。

- [ ] **Step 3: 实现**

`extension/src/sites/registry.ts` 整个文件替换为：

```ts
import type { SiteId } from "../shared/types";
import { chatgpt } from "./chatgpt";
import { claude } from "./claude";
import type { AdapterFactory } from "./types";

export interface SiteInfo {
  label: string;
  factory: AdapterFactory;
  origin: string;
  match: string;
  /** Checked against a real signed-in account. Unverified sites can be refreshed, read and exported, but never deleted from. */
  verified: boolean;
  /** The conversation id in a page path, in the form the adapter uses; null when the page is not a conversation. */
  idOfPath(path: string): string | null;
  /** The page of a conversation. */
  urlOf(id: string): string;
}

const firstGroup = (pattern: RegExp) => (path: string): string | null => pattern.exec(path)?.[1] ?? null;

export const SITES: Record<SiteId, SiteInfo> = {
  chatgpt: {
    label: "ChatGPT", factory: chatgpt, origin: "https://chatgpt.com", match: "https://chatgpt.com/*", verified: true,
    idOfPath: firstGroup(/\/c\/([A-Za-z0-9-]+)/), urlOf: (id) => `https://chatgpt.com/c/${id}`,
  },
  claude: {
    label: "Claude", factory: claude, origin: "https://claude.ai", match: "https://claude.ai/*", verified: true,
    idOfPath: firstGroup(/\/chat\/([A-Za-z0-9-]+)/), urlOf: (id) => `https://claude.ai/chat/${id}`,
  },
};

export function siteOfUrl(url: string): SiteId | null {
  const origin = (() => { try { return new URL(url).origin; } catch { return ""; } })();
  return (Object.keys(SITES) as SiteId[]).find((id) => SITES[id].origin === origin) ?? null;
}

export function conversationIdOfUrl(site: SiteId, url: string): string | null {
  const path = (() => { try { return new URL(url).pathname; } catch { return ""; } })();
  return SITES[site].idOfPath(path);
}

export function conversationUrl(site: SiteId, id: string): string {
  return SITES[site].urlOf(id);
}
```

- [ ] **Step 4: 运行测试与检查**

Run: `npx vitest run extension/src && npm run typecheck && npm run lint`
Expected: 全部 PASS。

- [ ] **Step 5: Commit**

```bash
git add extension/src/sites/registry.ts extension/src/sites/registry.test.ts
git commit -m "refactor(extension): keep each site's URL rules and verified flag in its registry entry"
```

---

### Task 3: Gemini batchexecute 辅助模块

接口（2026-09-19 只读实测，见任务末尾 Task 10 复核）：

- 所有调用：`POST https://gemini.google.com/_/BardChatUi/data/batchexecute?rpcids=<ID>&source-path=/app&bl=<bl>&f.sid=<fsid>&hl=en&_reqid=<5 位随机数>&rt=c`，表单 `f.req = JSON.stringify([[[ID, JSON.stringify(payload), null, "generic"]]])`、`at = <at>`。
- 令牌在页面 `WIZ_global_data` 中：`SNlM0e` = at（缺失即未登录）、`cfb2h` = bl、`FdrFJe` = f.sid、`S06Grb` = 用户编号。内容脚本读不到页面全局变量，所以从 `/app` HTML 用正则提取。`oPEP7c` 是邮箱，绝不读取。
- 回复为若干行，含 `"wrb.fr"` 的那一行是 JSON：找到 `[ "wrb.fr", <ID>, <JSON 字符串>, … ]`，解析第 3 项即为 rpc 的结果。

**Files:**
- Create: `extension/src/sites/batchexecute.ts`, `extension/src/sites/batchexecute.test.ts`, `extension/src/sites/fixtures/gemini.ts`

**Interfaces:**
- Consumes: `SiteError`。
- Produces（`sites/batchexecute.ts`）：

```ts
export const GEMINI_ORIGIN = "https://gemini.google.com";
export interface GeminiTokens { at: string; bl: string; fsid: string; userId: string }
export function extractTokens(html: string): GeminiTokens;          // E_AUTH without SNlM0e, E_BROKEN for other missing keys
export function batchUrl(rpcId: string, tokens: GeminiTokens, reqId: number): string;
export function batchForm(rpcId: string, payload: unknown, tokens: GeminiTokens): Record<string, string>;
export function parseBatch(text: string, rpcId: string): unknown;   // null when the rpc returned nothing; E_HTTP on an rpc error; E_BROKEN when no reply
```

- `fixtures/gemini.ts`：`appHtml`、`signedOutHtml`、`batchReply(rpcId, inner): string`、`errorReply(rpcId, code): string`（Task 4 在同一文件追加对话样本）。

- [ ] **Step 1: 写样本**

`extension/src/sites/fixtures/gemini.ts`：

```ts
/** Trimmed /app page: only the WIZ_global_data keys the adapter reads, plus the email it must never read. */
export const appHtml = '<!doctype html><html><head><script data-id="_gd" nonce="n">window.WIZ_global_data = {"oPEP7c":"someone@example.com","S06Grb":"108000000000000000001","SNlM0e":"AKlEn5_tok:1789000000000","cfb2h":"boq_assistant-bard-web-server_20260915.08_p0","FdrFJe":"-1234567890123456789","qwAQke":"BardChatUi"};</script></head><body></body></html>';

/** Signed-out /app page: no SNlM0e and no user id. */
export const signedOutHtml = '<!doctype html><html><head><script>window.WIZ_global_data = {"cfb2h":"boq_assistant-bard-web-server_20260915.08_p0","FdrFJe":"-1234567890123456789","qwAQke":"BardChatUi"};</script></head></html>';

/** A batchexecute reply as the site sends it: an anti-JSON prefix, then length-prefixed JSON lines. */
export function batchReply(rpcId: string, inner: unknown): string {
  const line = JSON.stringify([["wrb.fr", rpcId, JSON.stringify(inner), null, null, null, "generic"], ["di", 57], ["af.httprm", 56, "-3141592653589793238", 3]]);
  return `)]}'\n\n${line.length}\n${line}\n25\n[["e",4,null,null,${line.length + 30}]]\n`;
}

/** A batchexecute reply whose rpc failed: no result, a status array in position 5. */
export function errorReply(rpcId: string, code: number): string {
  const line = JSON.stringify([["wrb.fr", rpcId, null, null, null, [code], "generic"], ["di", 21]]);
  return `)]}'\n\n${line.length}\n${line}\n`;
}
```

- [ ] **Step 2: 写失败的测试**

`extension/src/sites/batchexecute.test.ts`：

```ts
import { describe, expect, it } from "vitest";
import { batchForm, batchUrl, extractTokens, parseBatch } from "./batchexecute";
import { appHtml, batchReply, errorReply, signedOutHtml } from "./fixtures/gemini";

const tokens = { at: "AKlEn5_tok:1789000000000", bl: "boq_assistant-bard-web-server_20260915.08_p0", fsid: "-1234567890123456789", userId: "108000000000000000001" };

describe("gemini batchexecute", () => {
  it("reads the page tokens and the user id from the /app HTML, never the email", () => {
    const got = extractTokens(appHtml);
    expect(got).toEqual(tokens);
    expect(JSON.stringify(got)).not.toContain("example.com");
  });

  it("decodes JSON escapes inside a token", () => {
    expect(extractTokens(appHtml.replace("AKlEn5_tok:1789000000000", "AKlEn5\\u003dtok")).at).toBe("AKlEn5=tok");
  });

  it("treats a page without the sign-in token as signed out, and other missing keys as a changed page", () => {
    expect(() => extractTokens(signedOutHtml)).toThrow("E_AUTH");
    expect(() => extractTokens(appHtml.replace('"cfb2h"', '"cfb2x"'))).toThrow("E_BROKEN: cfb2h");
    expect(() => extractTokens(appHtml.replace('"FdrFJe"', '"FdrFJx"'))).toThrow("E_BROKEN: FdrFJe");
    expect(() => extractTokens(appHtml.replace('"S06Grb"', '"S06Grx"'))).toThrow("E_BROKEN: S06Grb");
  });

  it("builds the request URL and form", () => {
    const url = new URL(batchUrl("MaZiqc", tokens, 12345));
    expect(url.origin + url.pathname).toBe("https://gemini.google.com/_/BardChatUi/data/batchexecute");
    expect(Object.fromEntries(url.searchParams)).toEqual({ rpcids: "MaZiqc", "source-path": "/app", bl: tokens.bl, "f.sid": tokens.fsid, hl: "en", _reqid: "12345", rt: "c" });
    const form = batchForm("MaZiqc", [100, null, [0, null, 1]], tokens);
    expect(form.at).toBe(tokens.at);
    expect(JSON.parse(form["f.req"])).toEqual([[["MaZiqc", "[100,null,[0,null,1]]", null, "generic"]]]);
  });

  it("finds the rpc's result among the reply lines", () => {
    expect(parseBatch(batchReply("MaZiqc", [null, "t", []]), "MaZiqc")).toEqual([null, "t", []]);
    expect(parseBatch(batchReply("GzXR5e", null), "GzXR5e")).toBeNull();
  });

  it("reports an rpc error as E_HTTP and a missing or unreadable reply as a changed interface", () => {
    expect(() => parseBatch(errorReply("GzXR5e", 3), "GzXR5e")).toThrow("E_HTTP");
    expect(() => parseBatch(batchReply("MaZiqc", []), "hNvQHb")).toThrow("E_BROKEN");
    expect(() => parseBatch("<html>sign in</html>", "MaZiqc")).toThrow("E_BROKEN");
  });
});
```

- [ ] **Step 3: 运行，确认失败**

Run: `npx vitest run extension/src/sites/batchexecute.test.ts`
Expected: FAIL（`./batchexecute` 不存在）。

- [ ] **Step 4: 实现**

`extension/src/sites/batchexecute.ts`：

```ts
import { SiteError } from "../shared/types";

export const GEMINI_ORIGIN = "https://gemini.google.com";

/** The page tokens every batchexecute call needs, and the signed-in user's id. */
export interface GeminiTokens { at: string; bl: string; fsid: string; userId: string }

/** One string value of WIZ_global_data as it appears in the page's HTML, with JSON escapes decoded. */
function field(html: string, key: string): string | null {
  const m = new RegExp(String.raw`"${key}":"((?:[^"\\]|\\.)*)"`).exec(html);
  if (!m) return null;
  try { return JSON.parse(`"${m[1]}"`) as string; } catch { return null; }
}

/**
 * Reads the tokens out of the /app HTML: a content script runs in an isolated world and cannot see the page's
 * WIZ_global_data. Only these four keys are read; the email (oPEP7c) never is.
 */
export function extractTokens(html: string): GeminiTokens {
  const at = field(html, "SNlM0e");
  if (!at) throw new SiteError("E_AUTH", "no SNlM0e");
  const need = (key: string): string => {
    const value = field(html, key);
    if (!value) throw new SiteError("E_BROKEN", key);
    return value;
  };
  return { at, bl: need("cfb2h"), fsid: need("FdrFJe"), userId: need("S06Grb") };
}

export function batchUrl(rpcId: string, tokens: GeminiTokens, reqId: number): string {
  const query = new URLSearchParams({ rpcids: rpcId, "source-path": "/app", bl: tokens.bl, "f.sid": tokens.fsid, hl: "en", _reqid: String(reqId), rt: "c" });
  return `${GEMINI_ORIGIN}/_/BardChatUi/data/batchexecute?${query}`;
}

export function batchForm(rpcId: string, payload: unknown, tokens: GeminiTokens): Record<string, string> {
  return { "f.req": JSON.stringify([[[rpcId, JSON.stringify(payload), null, "generic"]]]), at: tokens.at };
}

/** The rpc's result: the "wrb.fr" entry for `rpcId`, its JSON string parsed; null when the rpc returned nothing. */
export function parseBatch(text: string, rpcId: string): unknown {
  for (const line of text.split("\n")) {
    if (!line.includes('"wrb.fr"')) continue;
    let outer: unknown;
    try { outer = JSON.parse(line); } catch { continue; }
    if (!Array.isArray(outer)) continue;
    for (const entry of outer) {
      if (!Array.isArray(entry) || entry[0] !== "wrb.fr" || entry[1] !== rpcId) continue;
      if (typeof entry[2] === "string") {
        try { return JSON.parse(entry[2]); } catch { throw new SiteError("E_BROKEN", `${rpcId} reply`); }
      }
      if (Array.isArray(entry[5]) && entry[5].length) throw new SiteError("E_HTTP", `${rpcId} status ${JSON.stringify(entry[5])}`);
      return null;
    }
  }
  throw new SiteError("E_BROKEN", `${rpcId} reply`);
}
```

- [ ] **Step 5: 运行测试与检查**

Run: `npx vitest run extension/src && npm run typecheck && npm run lint`
Expected: 全部 PASS。

- [ ] **Step 6: Commit**

```bash
git add extension/src/sites/batchexecute.ts extension/src/sites/batchexecute.test.ts extension/src/sites/fixtures/gemini.ts
git commit -m "feat(extension): Gemini batchexecute tokens, requests and reply parsing"
```

---

### Task 4: Gemini 适配器

接口（2026-09-19 只读实测；删除与多轮分页未实测，Task 10 核对）：

- 列出：rpc `MaZiqc`，payload `[100, pageToken|null, [0, null, 1]]` → `[null, nextPageToken|null, chats[]]`；每条 `["c_…", title, null, null, null, [unixSeconds, nanos]]`。列表只有一个时间（最近活动），创建与更新时间都取它。置顶对话可能在另一个列表中，本期不处理（Task 10 核对）。
- 读取：rpc `hNvQHb`，payload `[chatId, 1000, pageToken|null, 1, [0], [4], null, 1]` → `inner[0]` 为轮次，**新的在前**；每轮 `turn[2][0][0]` 为用户输入、`turn[3][0][0][1][0]` 为模型回答（取第一个候选）、`turn[4]` 为 `[unixSeconds, nanos]`。`inner[1]` 为非空字符串时视为续页令牌，放入 payload 第 3 项再取，直到没有令牌、没有新轮次或令牌重复（最多 50 页）。结果为 `null` 视为对话不存在（`E_NOT_FOUND`）。回复中没有标题，`title` 为空串（导出与列表用索引里的标题）。
- 删除：rpc `GzXR5e`，payload `[chatId]`（未实测）。Gemini 没有归档。
- 账号：`remoteId` = `S06Grb`，`label` = `"Gemini"`（不读邮箱就拿不到显示名）。`account()` 每次重新取 `/app` 令牌。
- 令牌过期：batchexecute 返回 400 或 401/403 时，重新取一次令牌再试一次。
- 对话 ID 保留 `c_` 前缀；网址 `https://gemini.google.com/app/<去掉 c_ 的 ID>`。只支持浏览器的默认 Google 账号（`/app`），不处理 `/u/1/` 等其他登录账号。

**Files:**
- Create: `extension/src/sites/gemini.ts`, `extension/src/sites/gemini.test.ts`
- Modify: `extension/src/sites/fixtures/gemini.ts`, `extension/src/shared/types.ts`, `extension/src/sites/registry.ts`, `extension/src/sites/registry.test.ts`

**Interfaces:**
- Consumes: `GEMINI_ORIGIN`、`GeminiTokens`、`extractTokens`、`batchUrl`、`batchForm`、`parseBatch`（Task 3）；`FetchInit.form`、`FetchResult.text`（Task 1）；`SiteInfo`（Task 2）；`arr`、`optStr`、`str`、`expectOk`。
- Produces:

```ts
// shared/types.ts
export type SiteId = "chatgpt" | "claude" | "gemini";
// sites/gemini.ts
export const gemini: AdapterFactory;
export function geminiUrl(id: string): string; // "c_ab12" → "https://gemini.google.com/app/ab12"
// registry: SITES.gemini = { label: "Gemini", origin: "https://gemini.google.com", match: "https://gemini.google.com/*", verified: false, … }
```

- [ ] **Step 1: 追加样本**

在 `extension/src/sites/fixtures/gemini.ts` 末尾追加：

```ts
/** MaZiqc results: a first page with a next-page token, then the last page. */
export const listFirst = [null, "tok-2", [
  ["c_aaa111", "Trip plan", null, null, null, [1_789_000_000, 500_000_000]],
  ["c_bbb222", "", null, null, null, [1_788_000_000, 0]],
]];
export const listLast = [null, null, [["c_ccc333", "Old chat", null, null, null, [1_787_000_000, 0]]]];

/** hNvQHb turns, newest first as Gemini sends them. */
export const chatNewestFirst: unknown[][] = [
  [["c_aaa111", "r_2"], ["c_aaa111", "r_2", "rc_2"], [["And in winter?"], 1, null, 0], [[["rc_2", ["Snowy and quiet."]]]], [1_789_000_100, 0]],
  [["c_aaa111", "r_1"], ["c_aaa111", "r_1", "rc_1"], [["Where to go in Kyoto?"], 1, null, 0], [[["rc_1", ["Try Arashiyama."]]]], [1_789_000_000, 0]],
];
```

- [ ] **Step 2: 写失败的测试**

`extension/src/sites/gemini.test.ts`：

```ts
import { describe, expect, it, vi } from "vitest";
import type { FetchInit, FetchResult } from "./http";
import { gemini } from "./gemini";
import { appHtml, batchReply, chatNewestFirst, listFirst, listLast, signedOutHtml } from "./fixtures/gemini";

type Call = [string, FetchInit | undefined];
interface Rpc { id: string; payload: unknown }

const text = (body: string, status = 200): FetchResult => ({ status, json: null, retryAfter: null, text: body });

/** The rpc id and decoded payload of a batchexecute call. */
function rpcOf(init?: FetchInit): Rpc {
  const [[[id, payload]]] = JSON.parse(init?.form?.["f.req"] ?? "[[[]]]") as [[[string, string]]];
  return { id, payload: JSON.parse(payload) };
}

function fake(route: (rpc: Rpc) => string | FetchResult, html = appHtml, calls: Call[] = []) {
  return vi.fn(async (url: string, init?: FetchInit): Promise<FetchResult> => {
    calls.push([url, init]);
    if (url === "https://gemini.google.com/app") return text(html);
    const out = route(rpcOf(init));
    return typeof out === "string" ? text(out) : out;
  });
}

const routes = ({ id, payload }: Rpc): string => {
  const p = payload as unknown[];
  if (id === "MaZiqc") return batchReply(id, p[1] === "tok-2" ? listLast : listFirst);
  if (id === "hNvQHb") return batchReply(id, [chatNewestFirst, null]);
  if (id === "GzXR5e") return batchReply(id, null);
  return "";
};
const batchCalls = (calls: Call[]) => calls.filter(([url]) => url.includes("/batchexecute"));
const pageLoads = (calls: Call[]) => calls.filter(([url]) => url === "https://gemini.google.com/app");

describe("gemini adapter", () => {
  it("names the account by the user id from the page and never keeps the email", async () => {
    const account = await gemini(fake(routes)).account();
    expect(account).toEqual({ remoteId: "108000000000000000001", label: "Gemini" });
    expect(JSON.stringify(account)).not.toContain("example.com");
  });

  it("treats a page without the sign-in token as signed out", async () => {
    await expect(gemini(fake(routes, signedOutHtml)).account()).rejects.toThrow("E_AUTH");
    await expect(gemini(fake(routes, signedOutHtml)).list(null)).rejects.toThrow("E_AUTH");
  });

  it("lists page by page with the token Gemini returns, fetching the page tokens once", async () => {
    const calls: Call[] = [];
    const a = gemini(fake(routes, appHtml, calls));
    const first = await a.list(null);
    expect(first.items.map((i) => [i.id, i.title])).toEqual([["c_aaa111", "Trip plan"], ["c_bbb222", ""]]);
    expect(first.items[0]).toMatchObject({ createdAt: 1_789_000_000_500, updatedAt: 1_789_000_000_500, archived: false });
    expect(first.next).toBe("tok-2");
    const second = await a.list(first.next);
    expect(second).toEqual({ items: [{ id: "c_ccc333", title: "Old chat", createdAt: 1_787_000_000_000, updatedAt: 1_787_000_000_000, archived: false }], next: null });
    expect(batchCalls(calls).map(([, init]) => rpcOf(init))).toEqual([
      { id: "MaZiqc", payload: [100, null, [0, null, 1]] },
      { id: "MaZiqc", payload: [100, "tok-2", [0, null, 1]] },
    ]);
    expect(pageLoads(calls)).toHaveLength(1);
  });

  it("sends the tokens the way the page does", async () => {
    const calls: Call[] = [];
    await gemini(fake(routes, appHtml, calls)).list(null);
    const [url, init] = batchCalls(calls)[0];
    const query = new URL(url).searchParams;
    expect(query.get("rpcids")).toBe("MaZiqc");
    expect(query.get("bl")).toBe("boq_assistant-bard-web-server_20260915.08_p0");
    expect(query.get("f.sid")).toBe("-1234567890123456789");
    expect(query.get("_reqid")).toMatch(/^\d{5}$/);
    expect(init).toMatchObject({ method: "POST", form: { at: "AKlEn5_tok:1789000000000" }, headers: { "X-Same-Domain": "1" } });
  });

  it("reads the turns oldest first, prompt before answer", async () => {
    const body = await gemini(fake(routes)).read("c_aaa111");
    expect(body.messages.map((m) => [m.role, m.text])).toEqual([
      ["user", "Where to go in Kyoto?"], ["assistant", "Try Arashiyama."],
      ["user", "And in winter?"], ["assistant", "Snowy and quiet."],
    ]);
    expect(body.messages[0].at).toBe(1_789_000_000_000);
    expect(body.updatedAt).toBe(1_789_000_100_000);
    expect(body.id).toBe("c_aaa111");
  });

  it("follows a continuation token when a chat has more turns", async () => {
    const calls: Call[] = [];
    const paged = ({ id, payload }: Rpc): string => {
      const p = payload as unknown[];
      if (id !== "hNvQHb") return "";
      return batchReply(id, p[2] === "more-1" ? [[chatNewestFirst[1]], null] : [[chatNewestFirst[0]], "more-1"]);
    };
    const body = await gemini(fake(paged, appHtml, calls)).read("c_aaa111");
    expect(body.messages.map((m) => m.text)).toEqual(["Where to go in Kyoto?", "Try Arashiyama.", "And in winter?", "Snowy and quiet."]);
    expect(batchCalls(calls).map(([, init]) => rpcOf(init).payload)).toEqual([
      ["c_aaa111", 1000, null, 1, [0], [4], null, 1],
      ["c_aaa111", 1000, "more-1", 1, [0], [4], null, 1],
    ]);
  });

  it("reports a chat that returns nothing as not found", async () => {
    await expect(gemini(fake(({ id }) => batchReply(id, null))).read("c_gone")).rejects.toThrow("E_NOT_FOUND");
  });

  it("deletes with GzXR5e, has no archive, and links without the c_ prefix", async () => {
    const calls: Call[] = [];
    const a = gemini(fake(routes, appHtml, calls));
    await a.remove("c_aaa111");
    expect(batchCalls(calls).map(([, init]) => rpcOf(init))).toEqual([{ id: "GzXR5e", payload: ["c_aaa111"] }]);
    expect(a.archive).toBeUndefined();
    expect(a.conversationUrl("c_aaa111")).toBe("https://gemini.google.com/app/aaa111");
  });

  it("fetches fresh page tokens once when the old ones are refused", async () => {
    const calls: Call[] = [];
    let refusals = 1;
    const a = gemini(fake(({ id }) => (refusals-- > 0 ? text("", 400) : batchReply(id, listLast)), appHtml, calls));
    expect((await a.list(null)).items.map((i) => i.id)).toEqual(["c_ccc333"]);
    expect(pageLoads(calls)).toHaveLength(2);
    await expect(gemini(fake(() => text("", 400))).list(null)).rejects.toThrow("E_HTTP: 400");
  });

  it("flags a changed shape as broken", async () => {
    await expect(gemini(fake(() => batchReply("MaZiqc", { chats: [] }))).list(null)).rejects.toThrow("E_BROKEN");
    await expect(gemini(fake(() => batchReply("MaZiqc", [null, null, [["aaa111", "x", null, null, null, [1, 0]]]]))).list(null)).rejects.toThrow("E_BROKEN");
    await expect(gemini(fake(() => "<html></html>")).list(null)).rejects.toThrow("E_BROKEN");
    await expect(gemini(fake(() => batchReply("hNvQHb", [[chatNewestFirst[0].slice(0, 3)], null]))).read("c_aaa111")).rejects.toThrow("E_BROKEN");
  });
});
```

`extension/src/sites/registry.test.ts`：

- `SAMPLE_ID` 改为 `{ chatgpt: "abc-1", claude: "k1", gemini: "c_aaa111" }`。
- 「marks which sites were checked」里的期望改为 `{ chatgpt: true, claude: true, gemini: false }`。
- 在「recognises sites and conversation ids from URLs」末尾追加：

```ts
    expect(siteOfUrl("https://gemini.google.com/app/aaa111")).toBe("gemini");
    expect(conversationIdOfUrl("gemini", "https://gemini.google.com/app/aaa111?hl=en")).toBe("c_aaa111");
    expect(conversationIdOfUrl("gemini", "https://gemini.google.com/app")).toBeNull();
    expect(conversationUrl("gemini", "c_aaa111")).toBe("https://gemini.google.com/app/aaa111");
```

- [ ] **Step 3: 运行，确认失败**

Run: `npx vitest run extension/src/sites/gemini.test.ts extension/src/sites/registry.test.ts`
Expected: FAIL（`./gemini` 不存在；站点表没有 `gemini`）。

- [ ] **Step 4: 实现**

`extension/src/shared/types.ts` 第一行改为：

```ts
export type SiteId = "chatgpt" | "claude" | "gemini";
```

`extension/src/sites/gemini.ts`：

```ts
import { SiteError, type Message, type RemoteConversation } from "../shared/types";
import { batchForm, batchUrl, extractTokens, GEMINI_ORIGIN, parseBatch, type GeminiTokens } from "./batchexecute";
import { arr, optStr, str } from "./guards";
import { expectOk } from "./http";
import type { AdapterFactory } from "./types";

const PAGE = 100;
/** Turns asked for per read; chats seen live had far fewer, so one page is the usual case. */
const TURNS = 1000;
const MAX_READ_PAGES = 50;
const RPC = { list: "MaZiqc", read: "hNvQHb", remove: "GzXR5e" } as const;

/** Gemini's chat ids carry a "c_" prefix that its page URLs leave out. */
export function geminiUrl(id: string): string {
  return `${GEMINI_ORIGIN}/app/${id.replace(/^c_/, "")}`;
}

/** Follows array indexes, failing as E_BROKEN at the first step that is not an array. */
function dig(v: unknown, path: string, ...indexes: number[]): unknown {
  let cur = v;
  let at = path;
  for (const i of indexes) {
    cur = arr(cur, at)[i];
    at = `${at}[${i}]`;
  }
  return cur;
}

/** [unixSeconds, nanos] → milliseconds. */
function stamp(v: unknown, path: string): number {
  const [seconds, nanos] = arr(v, path);
  if (typeof seconds !== "number" || !Number.isFinite(seconds)) throw new SiteError("E_BROKEN", path);
  return seconds * 1000 + (typeof nanos === "number" ? Math.floor(nanos / 1e6) : 0);
}

/** One turn is the user's prompt and the first candidate of the model's answer. */
function turnMessages(turn: unknown, path: string): Message[] {
  const at = stamp(dig(turn, path, 4), `${path}[4]`);
  const prompt = str(dig(turn, path, 2, 0, 0), `${path}[2][0][0]`).trim();
  const answer = str(dig(turn, path, 3, 0, 0, 1, 0), `${path}[3][0][0][1][0]`).trim();
  const out: Message[] = [];
  if (prompt) out.push({ role: "user", text: prompt, at, attachments: [] });
  if (answer) out.push({ role: "assistant", text: answer, at, attachments: [] });
  return out;
}

export const gemini: AdapterFactory = (fetchJson) => {
  let tokens: GeminiTokens | null = null;

  async function loadTokens(): Promise<GeminiTokens> {
    const res = await fetchJson(`${GEMINI_ORIGIN}/app`);
    expectOk(res);
    tokens = extractTokens(res.text ?? "");
    return tokens;
  }

  async function rpc(id: string, payload: unknown, retried = false): Promise<unknown> {
    const current = tokens ?? (await loadTokens());
    const reqId = 10_000 + Math.floor(Math.random() * 90_000);
    const res = await fetchJson(batchUrl(id, current, reqId), { method: "POST", form: batchForm(id, payload, current), headers: { "X-Same-Domain": "1" } });
    try {
      expectOk(res);
    } catch (e) {
      // Page tokens expire while a tab stays open; the site then answers 400 or 401. Fetch fresh ones once.
      const stale = e instanceof SiteError && (e.code === "E_AUTH" || (e.code === "E_HTTP" && e.detail === "400"));
      if (!stale || retried) throw e;
      tokens = null;
      return rpc(id, payload, true);
    }
    return parseBatch(res.text ?? "", id);
  }

  return {
    site: "gemini",
    origin: GEMINI_ORIGIN,
    conversationUrl: geminiUrl,

    async account() {
      const fresh = await loadTokens();
      return { remoteId: fresh.userId, label: "Gemini" };
    },

    /** Cursor is the page token Gemini returned with the previous page. */
    async list(cursor) {
      const reply = arr(await rpc(RPC.list, [PAGE, cursor, [0, null, 1]]), "list");
      const chats = reply[2] == null ? [] : arr(reply[2], "list[2]");
      const items: RemoteConversation[] = chats.map((raw, i) => {
        const path = `list[2][${i}]`;
        const id = str(dig(raw, path, 0), `${path}[0]`);
        if (!id.startsWith("c_")) throw new SiteError("E_BROKEN", `${path}[0]`);
        // The listing carries one time only: the last activity.
        const at = stamp(dig(raw, path, 5), `${path}[5]`);
        return { id, title: optStr(dig(raw, path, 1)), createdAt: at, updatedAt: at, archived: false };
      });
      const next = typeof reply[1] === "string" && reply[1] ? reply[1] : null;
      return { items, next };
    },

    async read(id) {
      const turns: unknown[] = [];
      const pageTokens = new Set<string>();
      let pageToken: string | null = null;
      for (let n = 0; n < MAX_READ_PAGES; n++) {
        const reply = await rpc(RPC.read, [id, TURNS, pageToken, 1, [0], [4], null, 1]);
        if (reply == null) throw new SiteError("E_NOT_FOUND", id);
        const batch = dig(reply, "read", 0);
        const got = batch == null ? [] : arr(batch, "read[0]");
        turns.push(...got);
        const next = dig(reply, "read", 1);
        if (typeof next !== "string" || !next || !got.length || pageTokens.has(next)) break;
        pageTokens.add(next);
        pageToken = next;
      }
      // Gemini sends the newest turn first.
      const messages = turns.map((turn, i) => turnMessages(turn, `read[0][${i}]`)).reverse().flat();
      const updatedAt = messages.reduce((max, m) => Math.max(max, m.at ?? 0), 0);
      return { id, title: "", updatedAt, messages };
    },

    async remove(id) {
      await rpc(RPC.remove, [id]);
    },
  };
};
```

`extension/src/sites/registry.ts`：import 区追加 `import { gemini, geminiUrl } from "./gemini";`，并在 `SITES` 的 `claude` 条目之后追加：

```ts
  gemini: {
    label: "Gemini", factory: gemini, origin: "https://gemini.google.com", match: "https://gemini.google.com/*", verified: false,
    idOfPath: (path) => {
      const hex = /\/app\/([0-9a-f]+)/.exec(path)?.[1];
      return hex ? `c_${hex}` : null;
    },
    urlOf: geminiUrl,
  },
```

- [ ] **Step 5: 运行测试与检查**

Run: `npx vitest run extension/src && npm run typecheck && npm run lint`
Expected: 全部 PASS。

- [ ] **Step 6: Commit**

```bash
git add extension/src/shared/types.ts extension/src/sites/gemini.ts extension/src/sites/gemini.test.ts extension/src/sites/fixtures/gemini.ts extension/src/sites/registry.ts extension/src/sites/registry.test.ts
git commit -m "feat(extension): Gemini adapter (unverified)"
```

---

### Task 5: Grok 适配器

接口（未实测：用户未登录 Grok，按公开资料与样本实现，站点表 `verified: false`）：

- `GET /rest/app-chat/conversations?pageSize=60[&pageToken=…]` → `{ conversations: [{ conversationId, title, starred, createTime (ISO), modifyTime (ISO) }], nextPageToken }`；缺 `modifyTime` 时用 `createTime`。
- `GET /rest/app-chat/conversations/{id}/response-node` → `{ responseNodes: [{ responseId, sender, parentResponseId? }] }`；当前分支：从最后一个节点沿 `parentResponseId` 回溯到根。
- `POST /rest/app-chat/conversations/{id}/load-responses`，body `{ responseIds: [...] }` → `{ responses: [{ responseId, message, sender, createTime, parentResponseId?, fileAttachments? }] }`；`sender` 不区分大小写，`human`/`user` → user、`assistant` → assistant，其他 → `E_BROKEN`；附件取对象的 `fileName` 或 `name`。
- `DELETE /rest/app-chat/conversations/{id}`。Grok 本期不做归档。
- 账号：`GET /rest/app-chat/conversations?pageSize=1` 证明已登录（401/403 → `E_AUTH`）；`remoteId` 取 Cookie `x-userid`，没有时为 `"default"`；`label` 为 `"Grok"`。
- 回复里没有标题，`title` 为空串；`updatedAt` 取最后一条消息时间。

**Files:**
- Create: `extension/src/sites/grok.ts`, `extension/src/sites/grok.test.ts`, `extension/src/sites/fixtures/grok.ts`
- Modify: `extension/src/shared/types.ts`, `extension/src/sites/registry.ts`, `extension/src/sites/registry.test.ts`

**Interfaces:**
- Consumes: `PageAccess`、`NO_PAGE`（Task 1）；`SiteInfo`（Task 2）；`arr`、`obj`、`optStr`、`str`、`time`、`expectOk`、`FetchInit`。
- Produces:

```ts
// shared/types.ts
export type SiteId = "chatgpt" | "claude" | "gemini" | "grok";
// sites/grok.ts
export const grok: AdapterFactory;
export function currentBranchIds(nodes: Record<string, unknown>[]): string[];
// registry: SITES.grok = { label: "Grok", origin: "https://grok.com", match: "https://grok.com/*", verified: false, … }
```

- [ ] **Step 1: 写样本**

`extension/src/sites/fixtures/grok.ts`：

```ts
export const listFirst = {
  conversations: [
    { conversationId: "g-1", title: "Rust lifetimes", starred: false, createTime: "2026-09-01T10:00:00.000Z", modifyTime: "2026-09-02T10:00:00.000Z" },
    { conversationId: "g-2", title: "", starred: true, createTime: "2026-08-01T10:00:00.000Z" },
  ],
  nextPageToken: "p2",
};
export const listLast = {
  conversations: [{ conversationId: "g-3", title: "Old", starred: false, createTime: "2026-07-01T10:00:00.000Z", modifyTime: "2026-07-01T11:00:00.000Z" }],
};
/** r2old is an answer the user regenerated away from; the last node (r4) is the one on screen. */
export const responseNodes = {
  responseNodes: [
    { responseId: "r1", sender: "human" },
    { responseId: "r2old", sender: "assistant", parentResponseId: "r1" },
    { responseId: "r2", sender: "assistant", parentResponseId: "r1" },
    { responseId: "r3", sender: "human", parentResponseId: "r2" },
    { responseId: "r4", sender: "assistant", parentResponseId: "r3" },
  ],
};
export const responses = {
  responses: [
    { responseId: "r1", message: "Explain lifetimes", sender: "human", createTime: "2026-09-01T10:00:00.000Z", fileAttachments: [{ fileName: "main.rs" }] },
    { responseId: "r2", message: "A lifetime is a scope.", sender: "ASSISTANT", createTime: "2026-09-01T10:00:05.000Z", parentResponseId: "r1" },
    { responseId: "r3", message: "Example?", sender: "human", createTime: "2026-09-01T10:01:00.000Z", parentResponseId: "r2" },
    { responseId: "r4", message: "fn f<'a>(x: &'a str) {}", sender: "assistant", createTime: "2026-09-01T10:01:05.000Z", parentResponseId: "r3" },
  ],
};
```

- [ ] **Step 2: 写失败的测试**

`extension/src/sites/grok.test.ts`：

```ts
import { describe, expect, it, vi } from "vitest";
import type { FetchInit, FetchResult } from "./http";
import type { PageAccess } from "./page";
import { grok } from "./grok";
import { listFirst, listLast, responseNodes, responses } from "./fixtures/grok";

type Call = [string, FetchInit | undefined];

function fake(route: (url: string, init?: FetchInit) => unknown, calls: Call[] = []) {
  return vi.fn(async (url: string, init?: FetchInit): Promise<FetchResult> => {
    calls.push([url, init]);
    const json = route(url, init);
    return json === undefined ? { status: 404, json: null, retryAfter: null } : { status: 200, json, retryAfter: null };
  });
}
const routes = (url: string, init?: FetchInit) => {
  if (init?.method === "DELETE") return {};
  if (url.endsWith("/response-node")) return responseNodes;
  if (url.endsWith("/load-responses")) return responses;
  if (url.includes("pageToken=p2")) return listLast;
  if (url.includes("/rest/app-chat/conversations?")) return listFirst;
  return undefined;
};
const withCookies = (cookies: Record<string, string>): PageAccess => ({ cookie: (name) => cookies[name] ?? null, storage: () => null });

describe("grok adapter", () => {
  it("proves the sign-in with a one-item listing and names the account by the x-userid cookie", async () => {
    const calls: Call[] = [];
    expect(await grok(fake(routes, calls), withCookies({ "x-userid": "u-42" })).account()).toEqual({ remoteId: "u-42", label: "Grok" });
    expect(calls[0][0]).toBe("https://grok.com/rest/app-chat/conversations?pageSize=1");
    expect(await grok(fake(routes)).account()).toEqual({ remoteId: "default", label: "Grok" });
  });

  it("treats a refused listing as signed out", async () => {
    const refused = vi.fn(async (): Promise<FetchResult> => ({ status: 401, json: null, retryAfter: null }));
    await expect(grok(refused).account()).rejects.toThrow("E_AUTH");
  });

  it("pages with the token Grok returns", async () => {
    const calls: Call[] = [];
    const a = grok(fake(routes, calls));
    const first = await a.list(null);
    expect(first.items.map((i) => [i.id, i.title])).toEqual([["g-1", "Rust lifetimes"], ["g-2", ""]]);
    expect(first.items[0]).toMatchObject({ createdAt: Date.parse("2026-09-01T10:00:00.000Z"), updatedAt: Date.parse("2026-09-02T10:00:00.000Z"), archived: false });
    expect(first.items[1].updatedAt).toBe(first.items[1].createdAt);
    expect(first.next).toBe("p2");
    const second = await a.list(first.next);
    expect(second.items.map((i) => i.id)).toEqual(["g-3"]);
    expect(second.next).toBeNull();
    expect(calls.map(([url]) => url)).toEqual([
      "https://grok.com/rest/app-chat/conversations?pageSize=60",
      "https://grok.com/rest/app-chat/conversations?pageSize=60&pageToken=p2",
    ]);
  });

  it("reads only the branch ending at the last node", async () => {
    const calls: Call[] = [];
    const body = await grok(fake(routes, calls)).read("g-1");
    expect(body.messages.map((m) => [m.role, m.text])).toEqual([
      ["user", "Explain lifetimes"], ["assistant", "A lifetime is a scope."],
      ["user", "Example?"], ["assistant", "fn f<'a>(x: &'a str) {}"],
    ]);
    expect(body.messages[0].attachments).toEqual(["main.rs"]);
    expect(body.updatedAt).toBe(Date.parse("2026-09-01T10:01:05.000Z"));
    const load = calls.find(([url]) => url.endsWith("/load-responses"))!;
    expect(load[1]).toMatchObject({ method: "POST", body: { responseIds: ["r1", "r2", "r3", "r4"] } });
  });

  it("deletes with DELETE and has no archive", async () => {
    const calls: Call[] = [];
    const a = grok(fake(routes, calls));
    await a.remove("g-1");
    expect(calls).toEqual([["https://grok.com/rest/app-chat/conversations/g-1", { method: "DELETE" }]]);
    expect(a.archive).toBeUndefined();
    expect(a.conversationUrl("g-1")).toBe("https://grok.com/c/g-1");
  });

  it("flags a changed shape as broken", async () => {
    await expect(grok(fake(() => ({ items: [] }))).list(null)).rejects.toThrow("E_BROKEN");
    const oddSender = (url: string, init?: FetchInit) =>
      url.endsWith("/load-responses") ? { responses: responses.responses.map((r) => ({ ...r, sender: "system" })) } : routes(url, init);
    await expect(grok(fake(oddSender)).read("g-1")).rejects.toThrow("E_BROKEN");
    const missing = (url: string, init?: FetchInit) => (url.endsWith("/load-responses") ? { responses: [] } : routes(url, init));
    await expect(grok(fake(missing)).read("g-1")).rejects.toThrow("E_BROKEN");
    const dangling = (url: string, init?: FetchInit) =>
      url.endsWith("/response-node") ? { responseNodes: [{ responseId: "r9", sender: "human", parentResponseId: "nowhere" }] } : routes(url, init);
    await expect(grok(fake(dangling)).read("g-1")).rejects.toThrow("E_BROKEN");
  });
});
```

`extension/src/sites/registry.test.ts`：

- `SAMPLE_ID` 增加 `grok: "g-1"`。
- 「marks which sites were checked」期望改为 `{ chatgpt: true, claude: true, gemini: false, grok: false }`。
- 在「recognises sites and conversation ids from URLs」末尾追加：

```ts
    expect(siteOfUrl("https://grok.com/c/0f3e-11")).toBe("grok");
    expect(conversationIdOfUrl("grok", "https://grok.com/c/0f3e-11?rid=x")).toBe("0f3e-11");
    expect(conversationIdOfUrl("grok", "https://grok.com/")).toBeNull();
```

- [ ] **Step 3: 运行，确认失败**

Run: `npx vitest run extension/src/sites/grok.test.ts extension/src/sites/registry.test.ts`
Expected: FAIL（`./grok` 不存在；站点表没有 `grok`）。

- [ ] **Step 4: 实现**

`extension/src/shared/types.ts` 第一行改为：

```ts
export type SiteId = "chatgpt" | "claude" | "gemini" | "grok";
```

`extension/src/sites/grok.ts`：

```ts
import { SiteError, type Message, type RemoteConversation, type Role } from "../shared/types";
import { arr, obj, optStr, str, time } from "./guards";
import { expectOk, type FetchInit } from "./http";
import { NO_PAGE } from "./page";
import type { AdapterFactory } from "./types";

const ORIGIN = "https://grok.com";
const PAGE = 60;

function fileNames(v: unknown): string[] {
  if (!Array.isArray(v)) return [];
  return v
    .map((f) => (typeof f === "object" && f ? optStr((f as Record<string, unknown>).fileName) || optStr((f as Record<string, unknown>).name) : ""))
    .filter(Boolean);
}

function roleOf(sender: string, path: string): Role {
  const s = sender.toLowerCase();
  if (s === "human" || s === "user") return "user";
  if (s === "assistant") return "assistant";
  throw new SiteError("E_BROKEN", path);
}

/** Response ids from the root to the last node, which is the one on screen; other branches are left out. */
export function currentBranchIds(nodes: Record<string, unknown>[]): string[] {
  if (!nodes.length) return [];
  const parentOf = new Map(nodes.map((n, i) => [str(n.responseId, `responseNodes[${i}].responseId`), optStr(n.parentResponseId) || null]));
  const chain: string[] = [];
  let id: string | null = str(nodes[nodes.length - 1].responseId, "responseNodes[-1].responseId");
  while (id && !chain.includes(id)) {
    if (!parentOf.has(id)) throw new SiteError("E_BROKEN", `responseNodes.${id}`);
    chain.push(id);
    id = parentOf.get(id) ?? null;
  }
  return chain.reverse();
}

export const grok: AdapterFactory = (fetchJson, page = NO_PAGE) => {
  async function get(path: string, init: FetchInit = {}): Promise<unknown> {
    return expectOk(await fetchJson(`${ORIGIN}${path}`, init));
  }
  const conversationPath = (id: string) => `/rest/app-chat/conversations/${encodeURIComponent(id)}`;

  return {
    site: "grok",
    origin: ORIGIN,
    conversationUrl: (id) => `${ORIGIN}/c/${id}`,

    /** No known profile endpoint: a one-item listing proves the sign-in, the x-userid cookie names the account. */
    async account() {
      arr(obj(await get("/rest/app-chat/conversations?pageSize=1"), "conversations").conversations, "conversations.conversations");
      return { remoteId: page.cookie("x-userid") || "default", label: "Grok" };
    },

    /** Cursor is the page token Grok returned with the previous page. */
    async list(cursor) {
      const query = new URLSearchParams({ pageSize: String(PAGE) });
      if (cursor) query.set("pageToken", cursor);
      const res = obj(await get(`/rest/app-chat/conversations?${query}`), "page");
      const items: RemoteConversation[] = arr(res.conversations, "page.conversations").map((raw, i) => {
        const c = obj(raw, `conversations[${i}]`);
        const createdAt = time(c.createTime, `conversations[${i}].createTime`);
        return {
          id: str(c.conversationId, `conversations[${i}].conversationId`),
          title: optStr(c.title),
          createdAt,
          updatedAt: c.modifyTime == null ? createdAt : time(c.modifyTime, `conversations[${i}].modifyTime`),
          archived: false,
        };
      });
      return { items, next: optStr(res.nextPageToken) || null };
    },

    async read(id) {
      const tree = obj(await get(`${conversationPath(id)}/response-node`), "response-node");
      const nodes = arr(tree.responseNodes, "responseNodes").map((n, i) => obj(n, `responseNodes[${i}]`));
      const chain = currentBranchIds(nodes);
      if (!chain.length) return { id, title: "", updatedAt: 0, messages: [] };
      const loaded = obj(await get(`${conversationPath(id)}/load-responses`, { method: "POST", body: { responseIds: chain } }), "load-responses");
      const byId = new Map(arr(loaded.responses, "responses").map((r, i) => {
        const response = obj(r, `responses[${i}]`);
        return [str(response.responseId, `responses[${i}].responseId`), response] as const;
      }));
      const messages = chain.flatMap((rid): Message[] => {
        const r = byId.get(rid);
        if (!r) throw new SiteError("E_BROKEN", `responses.${rid}`);
        const role = roleOf(str(r.sender, `responses.${rid}.sender`), `responses.${rid}.sender`);
        const text = optStr(r.message).trim();
        const attachments = fileNames(r.fileAttachments);
        if (!text && !attachments.length) return [];
        return [{ role, text, at: r.createTime == null ? null : time(r.createTime, `responses.${rid}.createTime`), attachments }];
      });
      const updatedAt = messages.reduce((max, m) => Math.max(max, m.at ?? 0), 0);
      return { id, title: "", updatedAt, messages };
    },

    async remove(id) {
      await get(conversationPath(id), { method: "DELETE" });
    },
  };
};
```

`extension/src/sites/registry.ts`：import 区追加 `import { grok } from "./grok";`，在 `gemini` 条目之后追加：

```ts
  grok: {
    label: "Grok", factory: grok, origin: "https://grok.com", match: "https://grok.com/*", verified: false,
    idOfPath: firstGroup(/\/c\/([A-Za-z0-9-]+)/), urlOf: (id) => `https://grok.com/c/${id}`,
  },
```

- [ ] **Step 5: 运行测试与检查**

Run: `npx vitest run extension/src && npm run typecheck && npm run lint`
Expected: 全部 PASS。

- [ ] **Step 6: Commit**

```bash
git add extension/src/shared/types.ts extension/src/sites/grok.ts extension/src/sites/grok.test.ts extension/src/sites/fixtures/grok.ts extension/src/sites/registry.ts extension/src/sites/registry.test.ts
git commit -m "feat(extension): Grok adapter (unverified)"
```

---

### Task 6: DeepSeek 适配器

接口（未实测：用户未登录 DeepSeek，按公开资料与样本实现，站点表 `verified: false`）：

- 登录令牌：页面 localStorage `userToken` = `{"value":"<token>", …}`（内容脚本可读同源 localStorage）；每次请求时读取，不缓存、不写入插件数据。缺失或 `value` 为空 → `E_AUTH`；不是 JSON → `E_BROKEN`。请求头 `Authorization: Bearer <token>`、`x-client-platform: web`。
- 回复包装：`{ code: 0, msg, data: { biz_code: 0, biz_data } }`。HTTP 401/403 → `E_AUTH`；`code !== 0` 且 `msg` 与登录有关（匹配 `/token|auth|login|sign/i`）→ `E_AUTH`，否则 `E_BROKEN`；`data` 不是对象或 `biz_code !== 0` → `E_BROKEN`。
- 账号：`GET /api/v0/users/current` → `biz_data.id`（字符串或数字）；不保留邮箱与手机号；`label` 为 `"DeepSeek"`。
- 列出：`GET /api/v0/chat_session/fetch_page?count=100[&lte_cursor.pinned=<bool>&lte_cursor.updated_at=<float>]` → `{ chat_sessions: [{ id, title, pinned, updated_at, inserted_at }], has_more }`。游标为上一页最后一条的 `<pinned>|<updated_at>|<id>`；站点游标是「小于等于」，所以下一页里那一条要去掉；游标不前进时 → `E_BROKEN`（防止死循环）。
- 读取：`GET /api/v0/chat/history_messages?chat_session_id=<id>` → `{ chat_session: { id, title, updated_at, current_message_id }, chat_messages: [{ message_id, parent_id, role, content, inserted_at, files?, fragments? }] }`。当前分支：从 `current_message_id` 沿 `parent_id` 回溯（为 `null` 时按原顺序全取）。正文：有 `fragments` 时只取 `REQUEST` / `RESPONSE` 片段（不含深度思考），否则取 `content`。附件取 `files[].file_name`。
- 删除：`POST /api/v0/chat_session/delete`，body `{ chat_session_id }`。DeepSeek 本期不做归档。

**Files:**
- Create: `extension/src/sites/deepseek.ts`, `extension/src/sites/deepseek.test.ts`, `extension/src/sites/fixtures/deepseek.ts`
- Modify: `extension/src/shared/types.ts`, `extension/src/sites/registry.ts`, `extension/src/sites/registry.test.ts`

**Interfaces:**
- Consumes: `PageAccess`、`NO_PAGE`（Task 1）；`SiteInfo`（Task 2）；`arr`、`obj`、`optStr`、`str`、`time`、`expectOk`、`FetchInit`。
- Produces:

```ts
// shared/types.ts
export type SiteId = "chatgpt" | "claude" | "gemini" | "grok" | "deepseek";
// sites/deepseek.ts
export const deepseek: AdapterFactory;
// registry: SITES.deepseek = { label: "DeepSeek", origin: "https://chat.deepseek.com", match: "https://chat.deepseek.com/*", verified: false, … }
```

- [ ] **Step 1: 写样本**

`extension/src/sites/fixtures/deepseek.ts`：

```ts
export const ok = (bizData: unknown) => ({ code: 0, msg: "", data: { biz_code: 0, biz_msg: "", biz_data: bizData } });

export const user = ok({ id: "ds-user-1", email: "someone@example.com", mobile_number: "13800000000", token: "tok-ds" });

export const pageFirst = ok({
  chat_sessions: [
    { id: "s-1", title: "Pinned plan", pinned: true, updated_at: 1_789_000_000.25, inserted_at: 1_788_000_000.5 },
    { id: "s-2", title: "Sorting", pinned: false, updated_at: 1_788_900_000.75, inserted_at: 1_788_800_000 },
  ],
  has_more: true,
});
/** The site's cursor is inclusive: the last session of the previous page comes back first. */
export const pageLast = ok({
  chat_sessions: [
    { id: "s-2", title: "Sorting", pinned: false, updated_at: 1_788_900_000.75, inserted_at: 1_788_800_000 },
    { id: "s-3", title: "", pinned: false, updated_at: 1_788_000_000, inserted_at: 1_787_000_000 },
  ],
  has_more: false,
});

/** Message 2 is an answer the user regenerated away from; message 4 is the one on screen. */
export const history = ok({
  chat_session: { id: "s-1", title: "Pinned plan", updated_at: 1_789_000_000.25, current_message_id: 4 },
  chat_messages: [
    { message_id: 1, parent_id: null, role: "USER", content: "Plan a sprint", inserted_at: 1_788_000_000.5, files: [{ file_name: "backlog.csv" }] },
    { message_id: 2, parent_id: 1, role: "ASSISTANT", content: "Old answer", inserted_at: 1_788_000_010 },
    { message_id: 3, parent_id: 1, role: "ASSISTANT", content: "", thinking_content: "hmm", inserted_at: 1_788_000_020, fragments: [{ type: "THINK", content: "hmm" }, { type: "RESPONSE", content: "Two weeks, three goals." }] },
    { message_id: 4, parent_id: 3, role: "USER", content: "Shorter?", inserted_at: 1_788_000_030 },
  ],
});

export const signedOut = { code: 40003, msg: "Authorization Failed (invalid token)", data: null };
export const serverError = { code: 50000, msg: "Internal error", data: null };
```

- [ ] **Step 2: 写失败的测试**

`extension/src/sites/deepseek.test.ts`：

```ts
import { describe, expect, it, vi } from "vitest";
import type { FetchInit, FetchResult } from "./http";
import type { PageAccess } from "./page";
import { deepseek } from "./deepseek";
import { history, ok, pageFirst, pageLast, serverError, signedOut, user } from "./fixtures/deepseek";

type Call = [string, FetchInit | undefined];

function fake(route: (url: string, init?: FetchInit) => unknown, calls: Call[] = []) {
  return vi.fn(async (url: string, init?: FetchInit): Promise<FetchResult> => {
    calls.push([url, init]);
    const json = route(url, init);
    return json === undefined ? { status: 404, json: null, retryAfter: null } : { status: 200, json, retryAfter: null };
  });
}
const routes = (url: string) => {
  if (url.endsWith("/api/v0/users/current")) return user;
  if (url.includes("/chat_session/fetch_page")) return url.includes("lte_cursor") ? pageLast : pageFirst;
  if (url.includes("/chat/history_messages")) return history;
  if (url.endsWith("/chat_session/delete")) return ok(null);
  return undefined;
};
const signedIn: PageAccess = { cookie: () => null, storage: (key) => (key === "userToken" ? JSON.stringify({ value: "tok-ds", __version: "0" }) : null) };

describe("deepseek adapter", () => {
  it("names the account by user id, sends the page's token, and never keeps the email or phone", async () => {
    const calls: Call[] = [];
    const account = await deepseek(fake(routes, calls), signedIn).account();
    expect(account).toEqual({ remoteId: "ds-user-1", label: "DeepSeek" });
    expect(JSON.stringify(account)).not.toMatch(/example\.com|13800000000/);
    expect(calls[0][1]?.headers).toEqual({ Authorization: "Bearer tok-ds", "x-client-platform": "web" });
  });

  it("is signed out without a token, or when the site refuses it", async () => {
    const calls: Call[] = [];
    await expect(deepseek(fake(routes, calls)).account()).rejects.toThrow("E_AUTH");
    expect(calls).toEqual([]);
    await expect(deepseek(fake(() => signedOut), signedIn).account()).rejects.toThrow("E_AUTH");
    const refused = vi.fn(async (): Promise<FetchResult> => ({ status: 401, json: null, retryAfter: null }));
    await expect(deepseek(refused, signedIn).account()).rejects.toThrow("E_AUTH");
  });

  it("treats any other failure code or wrapper change as broken", async () => {
    await expect(deepseek(fake(() => serverError), signedIn).account()).rejects.toThrow("E_BROKEN");
    await expect(deepseek(fake(() => ({ code: 0, msg: "", data: { biz_code: 7, biz_data: null } })), signedIn).account()).rejects.toThrow("E_BROKEN");
    await expect(deepseek(fake(() => ok({ sessions: [] })), signedIn).list(null)).rejects.toThrow("E_BROKEN");
  });

  it("pages with the last session as cursor and drops the repeated boundary session", async () => {
    const calls: Call[] = [];
    const a = deepseek(fake(routes, calls), signedIn);
    const first = await a.list(null);
    expect(first.items.map((i) => [i.id, i.title])).toEqual([["s-1", "Pinned plan"], ["s-2", "Sorting"]]);
    expect(first.items[0]).toMatchObject({ createdAt: 1_788_000_000_500, updatedAt: 1_789_000_000_250, archived: false });
    expect(first.next).toBe("false|1788900000.75|s-2");
    const second = await a.list(first.next);
    expect(second.items.map((i) => i.id)).toEqual(["s-3"]);
    expect(second.next).toBeNull();
    const query = new URL(calls[1][0]).searchParams;
    expect([query.get("count"), query.get("lte_cursor.pinned"), query.get("lte_cursor.updated_at")]).toEqual(["100", "false", "1788900000.75"]);
  });

  it("stops when the cursor does not move", async () => {
    const stuck = deepseek(fake(() => pageFirst), signedIn);
    await expect(stuck.list("false|1788900000.75|s-2")).rejects.toThrow("E_BROKEN: paging");
  });

  it("reads the branch on screen, answers without the thinking", async () => {
    const body = await deepseek(fake(routes), signedIn).read("s-1");
    expect(body.messages.map((m) => [m.role, m.text])).toEqual([["user", "Plan a sprint"], ["assistant", "Two weeks, three goals."], ["user", "Shorter?"]]);
    expect(body.messages[0].attachments).toEqual(["backlog.csv"]);
    expect(body).toMatchObject({ id: "s-1", title: "Pinned plan", updatedAt: 1_789_000_000_250 });
  });

  it("fails on a current message that is not in the chat", async () => {
    const lost = ok({ chat_session: { id: "s-1", title: "", updated_at: 1, current_message_id: 99 }, chat_messages: [] });
    await expect(deepseek(fake(() => lost), signedIn).read("s-1")).rejects.toThrow("E_BROKEN");
  });

  it("deletes with POST and has no archive", async () => {
    const calls: Call[] = [];
    const a = deepseek(fake(routes, calls), signedIn);
    await a.remove("s-1");
    expect(calls[0][0]).toBe("https://chat.deepseek.com/api/v0/chat_session/delete");
    expect(calls[0][1]).toMatchObject({ method: "POST", body: { chat_session_id: "s-1" } });
    expect(a.archive).toBeUndefined();
    expect(a.conversationUrl("s-1")).toBe("https://chat.deepseek.com/a/chat/s/s-1");
  });
});
```

`extension/src/sites/registry.test.ts`：

- `SAMPLE_ID` 增加 `deepseek: "s-1"`。
- 「marks which sites were checked」期望改为 `{ chatgpt: true, claude: true, gemini: false, grok: false, deepseek: false }`。
- 在「recognises sites and conversation ids from URLs」末尾追加：

```ts
    expect(siteOfUrl("https://chat.deepseek.com/a/chat/s/7b2c-9")).toBe("deepseek");
    expect(conversationIdOfUrl("deepseek", "https://chat.deepseek.com/a/chat/s/7b2c-9")).toBe("7b2c-9");
    expect(conversationIdOfUrl("deepseek", "https://chat.deepseek.com/")).toBeNull();
```

- [ ] **Step 3: 运行，确认失败**

Run: `npx vitest run extension/src/sites/deepseek.test.ts extension/src/sites/registry.test.ts`
Expected: FAIL（`./deepseek` 不存在；站点表没有 `deepseek`）。

- [ ] **Step 4: 实现**

`extension/src/shared/types.ts` 第一行改为：

```ts
export type SiteId = "chatgpt" | "claude" | "gemini" | "grok" | "deepseek";
```

`extension/src/sites/deepseek.ts`：

```ts
import { SiteError, type Message, type RemoteConversation } from "../shared/types";
import { arr, obj, optStr, str, time } from "./guards";
import { expectOk, type FetchInit } from "./http";
import { NO_PAGE } from "./page";
import type { AdapterFactory } from "./types";

const ORIGIN = "https://chat.deepseek.com";
const PAGE = 100;
/** A failure code whose message is about the sign-in means the token is no longer accepted. */
const AUTH_MESSAGE = /token|auth|login|sign/i;

const idOf = (v: unknown, path: string): string => (typeof v === "number" && Number.isFinite(v) ? String(v) : str(v, path));

function fileNames(v: unknown): string[] {
  return Array.isArray(v) ? v.map((f) => optStr((f as Record<string, unknown>)?.file_name)).filter(Boolean) : [];
}

/** The request and the answer; the model's thinking and other fragments are left out. */
function messageText(m: Record<string, unknown>): string {
  if (Array.isArray(m.fragments) && m.fragments.length) {
    return m.fragments
      .map((f) => (typeof f === "object" && f ? (f as Record<string, unknown>) : {}))
      .filter((f) => f.type === "REQUEST" || f.type === "RESPONSE")
      .map((f) => optStr(f.content))
      .join("\n")
      .trim();
  }
  return optStr(m.content).trim();
}

/** The chain from the message on screen back to the root. */
function branch(messages: Record<string, unknown>[], leaf: string): Record<string, unknown>[] {
  const byId = new Map(messages.map((m, i) => [idOf(m.message_id, `chat_messages[${i}].message_id`), m]));
  const chain: Record<string, unknown>[] = [];
  const seen = new Set<string>();
  let id: string | null = leaf;
  while (id && !seen.has(id)) {
    const m = byId.get(id);
    if (!m) throw new SiteError("E_BROKEN", `chat_messages.${id}`);
    seen.add(id);
    chain.push(m);
    id = m.parent_id == null ? null : idOf(m.parent_id, `chat_messages.${id}.parent_id`);
  }
  return chain.reverse();
}

export const deepseek: AdapterFactory = (fetchJson, page = NO_PAGE) => {
  /** The page keeps its sign-in token in localStorage; it is read for each request and never stored by the extension. */
  function token(): string {
    const raw = page.storage("userToken");
    if (!raw) throw new SiteError("E_AUTH", "no userToken");
    let value: unknown;
    try { value = (JSON.parse(raw) as { value?: unknown } | null)?.value; } catch { throw new SiteError("E_BROKEN", "userToken"); }
    if (typeof value !== "string" || !value) throw new SiteError("E_AUTH", "no userToken");
    return value;
  }

  async function api(path: string, init: FetchInit = {}): Promise<unknown> {
    const headers = { ...init.headers, Authorization: `Bearer ${token()}`, "x-client-platform": "web" };
    const res = obj(expectOk(await fetchJson(`${ORIGIN}${path}`, { ...init, headers })), "response");
    if (res.code !== 0) {
      const msg = optStr(res.msg);
      throw new SiteError(AUTH_MESSAGE.test(msg) ? "E_AUTH" : "E_BROKEN", `code ${String(res.code)}${msg ? `: ${msg}` : ""}`);
    }
    const data = obj(res.data, "data");
    if (data.biz_code !== 0) throw new SiteError("E_BROKEN", `biz_code ${String(data.biz_code)}`);
    return data.biz_data;
  }

  return {
    site: "deepseek",
    origin: ORIGIN,
    conversationUrl: (id) => `${ORIGIN}/a/chat/s/${id}`,

    async account() {
      const me = obj(await api("/api/v0/users/current"), "users/current");
      return { remoteId: idOf(me.id, "users/current.id"), label: "DeepSeek" };
    },

    /** Cursor is `<pinned>|<updated_at>|<id>` of the previous page's last session; the site's cursor is inclusive, so that session is dropped. */
    async list(cursor) {
      const query = new URLSearchParams({ count: String(PAGE) });
      let boundary = "";
      if (cursor) {
        const [pinned, updatedAt, lastId] = cursor.split("|");
        query.set("lte_cursor.pinned", pinned);
        query.set("lte_cursor.updated_at", updatedAt);
        boundary = lastId ?? "";
      }
      const data = obj(await api(`/api/v0/chat_session/fetch_page?${query}`), "fetch_page");
      const rows = arr(data.chat_sessions, "chat_sessions").map((s, i) => obj(s, `chat_sessions[${i}]`));
      const items: RemoteConversation[] = rows
        .map((s, i) => {
          const updatedAt = time(s.updated_at, `chat_sessions[${i}].updated_at`);
          return {
            id: str(s.id, `chat_sessions[${i}].id`),
            title: optStr(s.title),
            createdAt: s.inserted_at == null ? updatedAt : time(s.inserted_at, `chat_sessions[${i}].inserted_at`),
            updatedAt,
            archived: false,
          };
        })
        .filter((c) => c.id !== boundary);
      const last = rows.at(-1);
      const next = data.has_more === true && last ? `${last.pinned === true}|${String(last.updated_at)}|${str(last.id, "chat_sessions[-1].id")}` : null;
      // An inclusive cursor that does not move would page forever.
      if (next !== null && next === cursor) throw new SiteError("E_BROKEN", "paging");
      return { items, next };
    },

    async read(id) {
      const data = obj(await api(`/api/v0/chat/history_messages?chat_session_id=${encodeURIComponent(id)}`), "history_messages");
      const session = obj(data.chat_session, "chat_session");
      const all = arr(data.chat_messages, "chat_messages").map((m, i) => obj(m, `chat_messages[${i}]`));
      const leaf = session.current_message_id == null ? null : idOf(session.current_message_id, "chat_session.current_message_id");
      const messages = (leaf ? branch(all, leaf) : all).flatMap((m): Message[] => {
        const path = `chat_messages.${String(m.message_id)}`;
        const role = str(m.role, `${path}.role`).toUpperCase();
        if (role !== "USER" && role !== "ASSISTANT") throw new SiteError("E_BROKEN", `${path}.role`);
        const text = messageText(m);
        const attachments = fileNames(m.files);
        if (!text && !attachments.length) return [];
        return [{ role: role === "USER" ? "user" : "assistant", text, at: m.inserted_at == null ? null : time(m.inserted_at, `${path}.inserted_at`), attachments }];
      });
      return { id, title: optStr(session.title), updatedAt: time(session.updated_at, "chat_session.updated_at"), messages };
    },

    async remove(id) {
      await api("/api/v0/chat_session/delete", { method: "POST", body: { chat_session_id: id } });
    },
  };
};
```

`extension/src/sites/registry.ts`：import 区追加 `import { deepseek } from "./deepseek";`，在 `grok` 条目之后追加：

```ts
  deepseek: {
    label: "DeepSeek", factory: deepseek, origin: "https://chat.deepseek.com", match: "https://chat.deepseek.com/*", verified: false,
    idOfPath: firstGroup(/\/a\/chat\/s\/([A-Za-z0-9-]+)/), urlOf: (id) => `https://chat.deepseek.com/a/chat/s/${id}`,
  },
```

- [ ] **Step 5: 运行测试与检查**

Run: `npx vitest run extension/src && npm run typecheck && npm run lint`
Expected: 全部 PASS。

- [ ] **Step 6: Commit**

```bash
git add extension/src/shared/types.ts extension/src/sites/deepseek.ts extension/src/sites/deepseek.test.ts extension/src/sites/fixtures/deepseek.ts extension/src/sites/registry.ts extension/src/sites/registry.test.ts
git commit -m "feat(extension): DeepSeek adapter (unverified)"
```

---

### Task 7: manifest 扩到 5 个站点

**Files:**
- Modify: `extension/public/manifest.json`, `extension/src/manifest.test.ts`

**Interfaces:**
- Consumes: `SITES`（Task 2、4、5、6；条目顺序 chatgpt、claude、gemini、grok、deepseek）。
- Produces: `host_permissions` 与 `content_scripts[0].matches` 恰为 5 个站点；版本 `0.2.0`。

- [ ] **Step 1: 写失败的测试**

`extension/src/manifest.test.ts` 整个文件替换为：

```ts
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
// @ts-expect-error plain ESM script without types
import { extensionId } from "../scripts/extension-id.mjs";
import { SITES } from "./sites/registry";

const manifest = JSON.parse(readFileSync(new URL("../public/manifest.json", import.meta.url), "utf8"));
const SITE_MATCHES = ["https://chatgpt.com/*", "https://claude.ai/*", "https://gemini.google.com/*", "https://grok.com/*", "https://chat.deepseek.com/*"];

describe("manifest", () => {
  it("asks only for the five supported sites and never for all URLs", () => {
    expect(manifest.host_permissions).toEqual(SITE_MATCHES);
    expect(manifest.content_scripts).toEqual([{ matches: SITE_MATCHES, js: ["content.js"], run_at: "document_idle" }]);
    expect(JSON.stringify(manifest)).not.toContain("<all_urls>");
    expect(manifest.permissions).toEqual(["storage", "downloads"]);
  });
  it("covers exactly the sites in the registry", () => {
    expect(Object.values(SITES).map((s) => s.match)).toEqual(SITE_MATCHES);
  });
  it("has a fixed ID recorded in EXTENSION_ID", () => {
    const recorded = readFileSync(new URL("../EXTENSION_ID", import.meta.url), "utf8").trim();
    expect(recorded).toMatch(/^[a-p]{32}$/);
    expect(extensionId(manifest.key)).toBe(recorded);
  });
});
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/manifest.test.ts`
Expected: FAIL（`host_permissions` 只有 2 项）。

- [ ] **Step 3: 实现**

`extension/public/manifest.json`（`key` 一字不改）改以下字段：

```json
  "version": "0.2.0",
  "description": "整理、搜索、导出和清理 ChatGPT、Claude、Gemini、Grok 与 DeepSeek 网页对话。数据只存在本机。",
  "host_permissions": ["https://chatgpt.com/*", "https://claude.ai/*", "https://gemini.google.com/*", "https://grok.com/*", "https://chat.deepseek.com/*"],
```

以及

```json
  "content_scripts": [
    { "matches": ["https://chatgpt.com/*", "https://claude.ai/*", "https://gemini.google.com/*", "https://grok.com/*", "https://chat.deepseek.com/*"], "js": ["content.js"], "run_at": "document_idle" }
  ]
```

- [ ] **Step 4: 运行测试、检查与构建**

Run: `npx vitest run extension/src && npm run typecheck && npm run lint && npm run ext:build`
Expected: 全部 PASS；`extension/dist/manifest.json` 含 5 个站点、版本 `0.2.0`。

- [ ] **Step 5: Commit**

```bash
git add extension/public/manifest.json extension/src/manifest.test.ts
git commit -m "feat(extension): run on Gemini, Grok and DeepSeek"
```

---

### Task 8: 管理页：未实测标记与禁止删除；5 站文案

未实测站点的对话在删除对话框里走已有的「按站点报错」机制（`siteErrors`）：该站点一行显示原因，这些对话不计入「将删除」，全部是未实测站点时删除按钮不可用。

**Files:**
- Create: `extension/src/ui/manage/siteStatus.ts`, `extension/src/ui/manage/siteStatus.test.ts`
- Modify: `extension/src/ui/manage/App.tsx`, `extension/src/ui/manage/DeleteDialog.test.tsx`, `extension/src/ui/popup/Popup.tsx`, `extension/src/i18n.ts`

**Interfaces:**
- Consumes: `SITES[site].verified`（Task 2）；`BrokenSites`（`lib/brokenSites.ts`）；`DeleteDialog` 的 `siteErrors: Partial<Record<SiteId, string>>`。
- Produces（`ui/manage/siteStatus.ts`）：

```ts
export function siteName(site: SiteId): string;                                     // "Grok（未实测）"
export function deleteBlockReason(site: SiteId, broken: BrokenSites): string | null; // null = can delete
```

- [ ] **Step 1: 写失败的测试**

`extension/src/ui/manage/siteStatus.test.ts`：

```ts
// @vitest-environment jsdom
Object.defineProperty(navigator, "language", { value: "zh-CN", configurable: true });
import { describe, expect, it } from "vitest";
import { deleteBlockReason, siteName } from "./siteStatus";

describe("site status", () => {
  it("blocks deleting on an unverified site whatever else is known", () => {
    expect(deleteBlockReason("grok", {})).toBe("未实测：该站点的接口还没有在真实账号上核对过，暂不支持删除");
    expect(deleteBlockReason("deepseek", { deepseek: 1 })).toBe("未实测：该站点的接口还没有在真实账号上核对过，暂不支持删除");
  });
  it("blocks a verified site only while its interface is marked changed", () => {
    expect(deleteBlockReason("chatgpt", {})).toBeNull();
    expect(deleteBlockReason("claude", { claude: 5 })).toBe("接口已变化，请先刷新该站点");
  });
  it("names unverified sites as such", () => {
    expect(siteName("chatgpt")).toBe("ChatGPT");
    expect(siteName("grok")).toBe("Grok（未实测）");
  });
});
```

`extension/src/ui/manage/DeleteDialog.test.tsx`：

- import 区追加 `import type { SiteId } from "../../shared/types";`。
- `conv` 的参数 `site: "chatgpt" | "claude" = "chatgpt"` 改为 `site: SiteId = "chatgpt"`。
- `mount` 的参数 `siteErrors: Partial<Record<"chatgpt" | "claude", string>> = {}` 改为 `siteErrors: Partial<Record<SiteId, string>> = {}`。
- 在 `describe` 内追加（它锁定未实测原因所依赖的机制，写完即应通过）：

```ts
  it("leaves out an unverified site's items and says why", async () => {
    const reason = "未实测：该站点的接口还没有在真实账号上核对过，暂不支持删除";
    const items = [conv("a"), conv("g", "grok:default", "grok")];
    const onRun = mount(vi.fn(async () => []), items, { grok: reason });
    const lines = [...host.querySelectorAll("ul.groups li")].map((li) => li.textContent);
    expect(lines).toEqual(["ChatGPT · 工作号：1 条", `Grok · grok:default：1 条 — ${reason}`]);
    expect(host.querySelector("ul.groups li:last-child span")?.className).toBe("err");
    await act(async () => button("删除 1 条").click());
    expect(onRun).toHaveBeenCalledWith([items[0]], "slim", expect.any(AbortSignal), expect.any(Function));
  });
```

- [ ] **Step 2: 运行，确认失败**

Run: `npx vitest run extension/src/ui/manage`
Expected: FAIL（`./siteStatus` 不存在）；新的 DeleteDialog 用例 PASS。

- [ ] **Step 3: 实现**

`extension/src/ui/manage/siteStatus.ts`：

```ts
import { t } from "../../i18n";
import type { BrokenSites } from "../../lib/brokenSites";
import type { SiteId } from "../../shared/types";
import { SITES } from "../../sites/registry";

/** The site's name in pickers; unverified sites say so. */
export function siteName(site: SiteId): string {
  return SITES[site].verified ? SITES[site].label : `${SITES[site].label}（${t("未实测")}）`;
}

/** Why the site's conversations cannot be deleted right now, or null when they can; shown per site in the delete dialog. */
export function deleteBlockReason(site: SiteId, broken: BrokenSites): string | null {
  if (!SITES[site].verified) return t("未实测：该站点的接口还没有在真实账号上核对过，暂不支持删除");
  if (broken[site]) return t("接口已变化，请先刷新该站点");
  return null;
}
```

`extension/src/ui/manage/App.tsx`：

1. import 区在 `import { Filters } from "./Filters";` 之后追加：

```tsx
import { deleteBlockReason, siteName } from "./siteStatus";
```

2. `openDelete` 里把

```tsx
      if (stillBroken[site]) { errors[site] = t("接口已变化，请先刷新该站点"); continue; }
```

改为

```tsx
      const blocked = deleteBlockReason(site, stillBroken);
      if (blocked) { errors[site] = blocked; continue; }
```

3. 把

```tsx
  const siteAccounts = accounts.filter((a) => !filter.site || a.site === filter.site);
```

改为

```tsx
  const siteAccounts = accounts.filter((a) => !filter.site || a.site === filter.site);
  const unverified = (Object.keys(SITES) as SiteId[]).filter((s) => !SITES[s].verified);
```

4. 站点下拉框把

```tsx
        {(Object.keys(SITES) as SiteId[]).map((s) => <option key={s} value={s}>{SITES[s].label}</option>)}
```

改为

```tsx
        {(Object.keys(SITES) as SiteId[]).map((s) => <option key={s} value={s}>{siteName(s)}</option>)}
```

5. 在「接口已变化」那一行（`filter((s) => broken[s])`）之后插入：

```tsx
      {unverified.length > 0 && <span className="warn" title={t("这些站点的接口还没有在真实账号上核对过：可以刷新、读取和导出，暂不支持删除。")}>{unverified.map((s) => SITES[s].label).join("、")}：{t("未实测")}</span>}
```

6. 空列表提示把

```tsx
      {shown.length === 0 ? <p className="mut">{t("没有对话。先打开并登录 ChatGPT 或 Claude，再点「刷新」。")}</p>
```

改为

```tsx
      {shown.length === 0 ? <p className="mut">{t("没有对话。先打开并登录 ChatGPT、Claude、Gemini、Grok 或 DeepSeek，再点「刷新」。")}</p>
```

（「刷新」按钮、「打开」链接和站点下拉框本来就遍历 `SITES`，自动覆盖 5 个站点。）

`extension/src/ui/popup/Popup.tsx`：把

```tsx
  if (!tab || !id) return <p style={{ padding: 12 }}>{t("在 ChatGPT 或 Claude 打开一条对话后，这里会显示它。")}</p>;
```

改为

```tsx
  if (!tab || !id) return <p style={{ padding: 12 }}>{t("在 ChatGPT、Claude、Gemini、Grok 或 DeepSeek 打开一条对话后，这里会显示它。")}</p>;
```

`extension/src/i18n.ts`：

- 删除不再使用的两条：`"没有对话。先打开并登录 ChatGPT 或 Claude，再点「刷新」。"` 与 `"在 ChatGPT 或 Claude 打开一条对话后，这里会显示它。"`。
- 在 `// manage/App.tsx` 一组末尾（`"接口已变化，请先刷新该站点"` 之后）追加：

```ts
  "没有对话。先打开并登录 ChatGPT、Claude、Gemini、Grok 或 DeepSeek，再点「刷新」。": "No conversations. Open and sign in to ChatGPT, Claude, Gemini, Grok or DeepSeek, then click “Refresh”.",
  "未实测": "not yet verified",
  "这些站点的接口还没有在真实账号上核对过：可以刷新、读取和导出，暂不支持删除。": "These sites' interfaces haven't been checked against a real account yet: refresh, read and export work; deleting is not available yet.",

  // manage/siteStatus.ts
  "未实测：该站点的接口还没有在真实账号上核对过，暂不支持删除": "Not yet verified: this site's interface hasn't been checked against a real account, so deleting is not available yet",
```

- 在 `// ui/popup/Popup.tsx` 一组开头追加：

```ts
  "在 ChatGPT、Claude、Gemini、Grok 或 DeepSeek 打开一条对话后，这里会显示它。": "Open a conversation on ChatGPT, Claude, Gemini, Grok or DeepSeek to see it here.",
```

- [ ] **Step 4: 运行测试、检查与构建**

Run: `npx vitest run extension/src && npm run typecheck && npm run lint && npm run ext:build`
Expected: 全部 PASS（含 `i18n.test.ts`）。

- [ ] **Step 5: Commit**

```bash
git add extension/src/ui/manage/siteStatus.ts extension/src/ui/manage/siteStatus.test.ts extension/src/ui/manage/App.tsx extension/src/ui/manage/DeleteDialog.test.tsx extension/src/ui/popup/Popup.tsx extension/src/i18n.ts
git commit -m "feat(extension): mark unverified sites and keep their conversations from being deleted"
```

---

### Task 9: 说明文档

**Files:**
- Modify: `extension/README.md`, `README.md`, `README.zh-CN.md`, `docs/superpowers/specs/2026-09-19-web-chat-extension-design.md`

**Interfaces:**
- Consumes: Task 4–8 的行为（站点、未实测、删除拦截、版本 0.2.0）。
- Produces: 文档，无代码接口。

- [ ] **Step 1: 改写插件说明**

`extension/README.md` 整个文件替换为：

````markdown
# Stacker 网页对话

管理 ChatGPT（chatgpt.com）、Claude（claude.ai）、Gemini（gemini.google.com）、Grok（grok.com）与 DeepSeek（chat.deepseek.com）网页版对话的浏览器插件：整理、搜索、导出、清理。

## 原则

- 数据只存在本机浏览器（IndexedDB），不上传、不同步到任何服务器。
- 不读取、不保存任何账号凭证；正文读取和删除都是借用你在浏览器里已经登录的状态，直接调用站点自己的网页接口完成的。ChatGPT 的访问令牌、Gemini 的页面令牌、DeepSeek 的登录令牌只在该网站页面里的插件脚本内存中临时使用，不写入插件数据。
- 不读取、不保存账号邮箱。
- 插件只申请上面 5 个网站的访问权限，不申请「所有网站」。
- 插件对网页本身的改动只有一处：选中文字时出现的「存为摘录」按钮。

## 安装

1. 构建插件：

   ```powershell
   npm run ext:build
   ```

   产物在 `extension/dist`。

2. 打开浏览器的扩展管理页：Chrome 访问 `chrome://extensions`，Edge 访问 `edge://extensions`。
3. 打开右上角的「开发者模式」。
4. 点击「加载已解压的扩展程序」，选择 `extension/dist` 目录。
5. 插件的 ID 固定为 `extension/EXTENSION_ID` 文件中的值（由 `manifest.json` 里内置的 `key` 决定，每次构建都一样）。
6. 浏览器可能会提示「已停用以开发者模式加载的扩展」，此时点「保留」即可继续使用。

安装或更新后，已经打开的上述网站页面请刷新一次，插件的内容脚本才会生效。从旧版本更新后，浏览器会请求新增 3 个网站的访问权限。

## 使用

1. 打开并登录要管理的网站（未登录时读取和删除都会失败并提示重新登录）。
2. 点击浏览器工具栏上的插件图标，打开管理页（一个独立标签页）。
3. 管理页顶部每个网站各有一个「刷新」按钮和一个「打开」链接：
   - 「刷新」只读取对话标题、时间等索引信息，不读正文；读取速度快、不消耗额度。
   - 「打开」在新标签页打开对应网站，方便先登录再回来刷新。
4. 正文（对话内容）按需读取：打开一条对话查看详情、导出、或删除前需要正文时才会去请求；读过的正文会缓存，避免重复请求。
5. 管理页支持按站点、账号筛选，按文件夹、标签整理，按标题或正文搜索，收藏、加备注。
6. `Alt+Shift+S`（可在浏览器快捷键设置里修改）打开一个可拖动、可调整大小的独立小窗口，显示当前浏览器标签页对应的那条对话：打标签、写备注、收藏、查看和管理摘录。
7. 在网页上选中一段文字，会出现「存为摘录」按钮，点一下把这段文字保存下来（在小窗口的摘录列表里查看）。

## 已实测与未实测的站点

| 站点 | 状态 | 刷新、读取、搜索、整理、导出 | 删除 |
|---|---|---|---|
| ChatGPT | 已实测 | 可以 | 可以 |
| Claude | 已实测 | 可以 | 可以 |
| Gemini | 未实测 | 可以 | 暂不可以 |
| Grok | 未实测 | 可以 | 暂不可以 |
| DeepSeek | 未实测 | 可以 | 暂不可以 |

「未实测」表示这个站点的网页接口还没有在真实账号上核对过（插件按公开资料实现，并且每次调用都会校验返回结构）。管理页的站点下拉框和顶部会标出「未实测」；删除对话框会列出这些站点的对话并说明「暂不支持删除」，不会删除它们。等插件在真实账号上核对通过后，会在后续版本开放删除。

各站点的差异：

- **Gemini**：只支持浏览器里默认的 Google 账号（`gemini.google.com/app`，不含 `/u/1/` 这类其他已登录账号）。列表只给出最近活动时间，所以创建时间与更新时间相同。账号显示为「Gemini」（不读邮箱就拿不到账号名，可以在管理页「改备注名」）。置顶的对话可能不在列表中。Gemini 没有归档。
- **Grok**：账号以网站的 `x-userid` Cookie 区分，没有时显示为 `default`。
- **DeepSeek**：读取正文时只保存提问和回答，不包含「深度思考」的内容。

## 导出

选中一条或多条对话后可以「导出精简版」（仅用户和助手的正文）或「导出完整版」（包含角色、时间、附件文件名等完整信息），导出为 Markdown 文件，保存到系统下载目录下的 `Stacker 网页对话/<站点>/` 文件夹，按日期、标题和对话 ID 命名，不会询问保存位置。

## 删除

选中要删除的对话后点「删除…」，可以选择：

- **精简导出后删除（默认）**：先把正文存成 Markdown，再删除网站上的对话。
- **完整备份后删除**：先保存当前分支的完整 Markdown（含工具消息和附件名），并把同样的消息另存为 JSON，再删除网站上的对话。不保存网站接口的原始数据。
- **直接删除**：不留任何副本，需要二次确认。

删除过程中：

- 只删除已实测站点、当前浏览器已登录账号下的对话；选中的对话如果属于其他账号，会被跳过，并提示先在网站上切换账号；属于未实测站点的对话不会被删除。
- 每处理 20 条会重新核对一次当前登录账号，账号在中途被切换会立刻停止继续删除那些不属于新账号的对话。
- 请求之间会自动限速；遇到网站返回「请求过于频繁」会自动退避重试。
- 删除中可以点「中止」随时停止，已经处理的条目不会回滚。
- 删除完成后会显示每一条的结果（成功、失败、跳过及原因）。

## 站点接口变化时的表现

这 5 个网站用的都是网站自己的私有接口，不是公开 API。如果网站改版导致某个接口返回的数据结构和插件预期的不一样，插件会立刻停止当前操作（刷新、读取、删除等），并提示「该网站接口已变化，已停止操作，等待插件更新」，不会继续按错误的数据处理，也不会误删。遇到这种情况请等待插件更新后重试。

## 打包

```powershell
npm run ext:zip
```

先执行 `ext:build`，再用 `extension/scripts/zip.mjs`（基于 PowerShell 自带的 `Compress-Archive`）把 `extension/dist` 打包成 `extension/stacker-web-chats-<version>.zip`，供离线分发或手动安装。
````

- [ ] **Step 2: 根目录说明与设计状态**

`README.md` 第 132 行中的 `manages ChatGPT and Claude web conversations` 改为 `manages ChatGPT, Claude, Gemini, Grok and DeepSeek web conversations`。

`README.zh-CN.md` 第 132 行中的 `ChatGPT、Claude 网页版对话` 改为 `ChatGPT、Claude、Gemini、Grok、DeepSeek 网页版对话`。

`docs/superpowers/specs/2026-09-19-web-chat-extension-design.md` 第 4 行 `状态：G1 已实现` 改为 `状态：G1、G2 已实现（Grok、DeepSeek 未实测，暂不支持删除）`。

- [ ] **Step 3: 检查**

Run: `npx vitest run extension/src && npm run typecheck && npm run lint && npm run ext:zip`
Expected: 全部 PASS；输出 `extension/stacker-web-chats-0.2.0.zip`。

- [ ] **Step 4: Commit**

```bash
git add extension/README.md README.md README.zh-CN.md docs/superpowers/specs/2026-09-19-web-chat-extension-design.md
git commit -m "docs(extension): Gemini, Grok and DeepSeek, and what unverified means"
```

---

### Task 10: 实机核对（协调者完成，不交给子代理）

本任务由协调者与用户一起完成。用户当前只登录了 Gemini；Grok、DeepSeek 未登录，绝不替用户登录，它们本期保持未实测。所有页面内操作用 Claude in Chrome 在用户的 Chrome 中进行；只记录字段名、类型与状态码，不输出令牌、邮箱、对话标题或内容；绝不读取 `WIZ_global_data.oPEP7c`。

- [ ] **Step 1: Gemini 只读核对（征得用户同意后）**

在已登录的 `https://gemini.google.com/app` 标签页中执行页面脚本，复用适配器的做法：`fetch("/app")` 取 HTML，用 `"SNlM0e":"…"`、`"cfb2h":"…"`、`"FdrFJe":"…"`、`"S06Grb":"…"` 正则提取（只输出每项是否存在与长度），再按 Task 3 的格式发 batchexecute：

- `MaZiqc`，payload `[3, null, [0, null, 1]]`：核对 `inner[1]` 为字符串或 `null`、`inner[2]` 为数组、每条 `[0]` 以 `c_` 开头、`[1]` 为字符串、`[5]` 为 `[number, number]`；再用返回的令牌取第二页，确认翻页可用。
- 若用户有置顶对话：问用户置顶对话的数量，核对它们是否出现在上述列表中。
- `hNvQHb`，payload `[<一条对话 ID>, 1000, null, 1, [0], [4], null, 1]`：核对 `inner[0]` 为数组、`turn[2][0][0]` 与 `turn[3][0][0][1][0]` 为字符串、`turn[4]` 为 `[number, number]`，记录 `inner[1]` 的类型。若用户有超过 10 轮的长对话，也读一次，核对是否一次返回全部轮次，或 `inner[1]` 给出续页令牌。

形状与样本不同时：更新 `fixtures/gemini.ts`、调整 `gemini.ts` 与测试（对外接口不变），跑 `npx vitest run extension/src && npm run typecheck && npm run lint`，提交 `fix(extension): match Gemini responses`。置顶对话不在列表中时，不修改 `verified`，在汇报里说明并请用户决定是否先补上置顶列表。

- [ ] **Step 2: 用户加载 0.2.0 后的界面核对**

用户在 `chrome://extensions` 重新加载 `extension/dist`（Task 7 之后已构建）并刷新 Gemini 页面。核对：管理页顶部有 5 个「刷新」按钮和「打开」链接，并显示「Gemini、Grok、DeepSeek：未实测」；「刷新 Gemini」后条数与网页侧栏一致；读取两三条正文，顺序为从旧到新、先问后答；导出精简版正常；选中一条 Gemini 对话点「删除…」，对话框该行显示「未实测：…暂不支持删除」，删除按钮为「删除 0 条」且不可用。

- [ ] **Step 3: 核对 Gemini 删除（仅针对用户同意新建的测试对话）**

先征得用户同意，由用户（或经用户同意由协调者）在 Gemini 新建 2 条测试对话，提问内容分别为「Stacker 删除测试 1」「Stacker 删除测试 2」，记下它们的网址。

1. 直接调用：在 Gemini 页面对测试对话 1 发 `GzXR5e`，payload `["c_<测试对话 1 的十六进制 ID>"]`。记录 HTTP 状态、是否有 `"wrb.fr","GzXR5e"` 条目、其第 3 项是字符串还是 `null`、第 6 项是否为错误数组。然后用 `MaZiqc` 确认它已不在列表中，刷新网页确认侧栏里也没有了，并对它发一次 `hNvQHb` 记录返回（结果为 `null` 还是错误条目，用于核对 `E_NOT_FOUND` 的判断）。
2. 通过插件删除：临时把 `extension/src/sites/registry.ts` 里 `gemini` 的 `verified` 改为 `true`，`npm run ext:build`，请用户重新加载插件；在管理页刷新 Gemini，只选中测试对话 2，用「精简导出后删除」删除。核对：下载目录 `Stacker 网页对话/gemini/` 下有它的 Markdown 且内容完整；网页侧栏里已消失；管理页该条标为已删除，结果为成功。

两步都通过：把 `extension/src/sites/registry.test.ts` 中「marks which sites were checked」的期望改为 `{ chatgpt: true, claude: true, gemini: true, grok: false, deepseek: false }`；`extension/README.md` 表格中 Gemini 一行改为 `| Gemini | 已实测 | 可以 | 可以 |`，若 Step 1 确认置顶对话在列表中，删去 Gemini 说明里「置顶的对话可能不在列表中。」一句；`docs/superpowers/specs/2026-09-19-web-chat-extension-design.md` 状态行保持「Grok、DeepSeek 未实测」。跑 `npx vitest run extension/src && npm run typecheck && npm run lint && npm run ext:build`，提交：

```bash
git add extension/src/sites/registry.ts extension/src/sites/registry.test.ts extension/README.md
git commit -m "feat(extension): enable deleting on Gemini after the live check"
```

任一步失败：把 `verified` 改回 `false`（`git checkout extension/src/sites/registry.ts`），按观察到的回复修正 `gemini.ts` / `batchexecute.ts` 与测试后再按本步重测；无法当场修正则保持未实测，在汇报中写明原因。若测试对话仍留在网站上，告诉用户由其自行删除。

- [ ] **Step 4: Grok、DeepSeek**

不做任何网站请求，保持 `verified: false`。只核对管理页显示它们为「未实测」。

- [ ] **Step 5: 汇报**

向用户汇报：Gemini 只读核对结果与任何形状差异及修复；置顶对话是否在列表中；删除核对结果与 `verified` 是否已改为 `true`；Grok、DeepSeek 仍未实测，待用户自行登录后在后续版本核对。

---

## Self-Review

- **Spec 覆盖（G2 范围）**：Gemini、Grok、DeepSeek 适配器，统一接口 `account/list/read/remove`（Task 4、5、6）；账号取内部编号、不取邮箱（Gemini `S06Grb`、Grok `x-userid`、DeepSeek `users/current.id`，测试断言不含邮箱）；每次调用校验结构、不符即 `E_BROKEN`（各适配器 “flags a changed shape” 用例）；Gemini batchexecute 单独模块及测试（Task 3）；借用页面登录状态、不保存凭证（Task 1 `PageAccess`，令牌只在内存）；只申请 5 个站点权限（Task 7）；按需读正文、分支只取当前显示分支（Grok、DeepSeek 回溯，Gemini 取第一个候选）；未实测站点显示「未实测」并禁止删除（Task 2 `verified`、Task 8）；站点切换、刷新、链接覆盖 5 站（Task 8，原本遍历 `SITES`）；i18n（Task 8）；README（Task 9）；实机只读核对、删除只测测试对话（Task 10）。G3、G4 不在本计划。
- **占位扫描**：每个代码步骤都给出完整代码或确切的替换前后文本；没有 “TBD”、“类似 Task N”。
- **类型一致**：`FetchInit.form`、`FetchResult.text`、`PageAccess`、`NO_PAGE`、`pageOf`（Task 1）在 Task 4–6 中同名使用；`SiteInfo` 的 `verified`、`idOfPath`、`urlOf`（Task 2）在 Task 4–8 中一致；`GeminiTokens`、`extractTokens`、`batchUrl`、`batchForm`、`parseBatch`、`GEMINI_ORIGIN`（Task 3）与 Task 4 一致；`geminiUrl` 由 Task 4 导出并用于站点表；`SiteId` 在 Task 4、5、6 依次扩展，站点表条目顺序 chatgpt、claude、gemini、grok、deepseek 与 Task 7 的 `SITE_MATCHES` 一致；`deleteBlockReason`、`siteName`（Task 8）只在 `App.tsx` 使用；`registry.test.ts` 的 `SAMPLE_ID` 与 `verified` 期望在每个任务里同步更新。
