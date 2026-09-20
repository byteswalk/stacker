/**
 * Design preview: the real manage page and popup in an ordinary browser tab, on sample data.
 * Used to check the layout without loading the extension; never part of a build (see
 * vite.config.ts inputs). Run `npm run ext:preview` and open the printed address.
 */
import "./previewChrome";
import { createRoot } from "react-dom/client";
import { addExcerpt, createFolder, mergeListing, openDb, putBody, updateLocal, upsertAccount, type Db } from "../lib/db";
import type { SiteId } from "../shared/types";
import { session } from "./previewChrome";
import { App } from "./manage/App";
import { Popup } from "./popup/Popup";
import { Shell } from "./Shell";

const DAY = 86_400_000;
const CHATS: { site: SiteId; remote: string; id: string; title: string; age: number }[] = [
  { site: "chatgpt", remote: "u1", id: "c-arc-rc", title: "Rust 里 Arc 和 Rc 的选择", age: 1 },
  { site: "chatgpt", remote: "u1", id: "c-hangzhou", title: "周末杭州两日行程", age: 3 },
  { site: "chatgpt", remote: "u2", id: "c-review", title: "季度复盘的结构怎么写", age: 6 },
  { site: "claude", remote: "c1", id: "c-skills", title: "Claude 必装 skill 推荐与 token 优化", age: 0 },
  { site: "claude", remote: "c1", id: "c-sql", title: "把这段 SQL 改成窗口函数", age: 2 },
  { site: "gemini", remote: "g1", id: "c-figure", title: "论文里的图表配色建议", age: 4 },
  { site: "grok", remote: "x1", id: "c-source", title: "这条新闻的原始出处是什么", age: 9 },
  { site: "deepseek", remote: "d1", id: "c-json", title: "用 Python 解析大 JSON 的省内存写法", age: 12 },
];

async function seed(db: Db) {
  const work = await createFolder(db, "工作", Date.now());
  await createFolder(db, "学习", Date.now());
  const owners = [...new Set(CHATS.map((c) => `${c.site}:${c.remote}`))];
  for (const owner of owners) {
    const [site, remote] = owner.split(":") as [SiteId, string];
    const account = await upsertAccount(db, site, { remoteId: remote, label: remote }, Date.now());
    const items = CHATS.filter((c) => `${c.site}:${c.remote}` === owner).map((c) => {
      const at = Date.now() - c.age * DAY;
      return { id: c.id, title: c.title, createdAt: at - DAY, updatedAt: at, archived: false };
    });
    await mergeListing(db, account, items, true, Date.now());
  }
  await updateLocal(db, ["chatgpt:c-hangzhou"], { favorite: true, tags: ["出行"], folderId: work.id, note: "记得订周五的票" });
  await updateLocal(db, ["claude:c-skills"], { tags: ["工具", "省钱"], note: "值得整理成团队规范" });
  await putBody(db, "claude:c-skills", {
    id: "c-skills", title: "Claude 必装 skill 推荐与 token 优化", updatedAt: Date.now(),
    messages: [
      { role: "user", text: "有哪些 skill 是值得常驻的？顺便说说怎么少烧点 token。", at: Date.now(), attachments: [] },
      { role: "assistant", text: "常驻三类就够：\n1. 写计划和拆任务的；\n2. 系统排查问题的；\n3. 代码审查的。\n\n省 token 的关键不是压缩提问，而是别让模型反复读同样的上下文。", at: Date.now(), attachments: [] },
    ],
  }, Date.now());
  await addExcerpt(db, {
    site: "claude", conversationId: "c-skills", url: "https://claude.ai/chat/c-skills",
    pageTitle: "Claude", text: "省 token 的关键不是压缩提问，而是别让模型反复读同样的上下文。", note: "写进团队规范",
  }, Date.now());
}

const which = new URLSearchParams(location.search).get("page");
if (which === "popup") {
  session.set("lastSiteTab", { tabId: 1, site: "claude", url: "https://claude.ai/chat/c-skills", title: "Claude 必装 skill 推荐与 token 优化" });
}

void openDb().then(async (db) => {
  if ((await db.count("conversations")) === 0) await seed(db);
  createRoot(document.getElementById("root")!).render(<Shell>{which === "popup" ? <Popup /> : <App />}</Shell>);
});
