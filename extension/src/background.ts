import { addExcerpt, openDb } from "./lib/db";
import { LAST_TAB_KEY, shouldRemember, siteTabOf } from "./lib/lastTab";
import { isValidSaveExcerpt } from "./lib/saveExcerpt";

export type { SaveExcerpt } from "./lib/saveExcerpt";

const MANAGE = chrome.runtime.getURL("manage.html");

chrome.action.onClicked.addListener(async () => {
  const contexts = await chrome.runtime.getContexts({ contextTypes: ["TAB"], documentUrls: [MANAGE] });
  const ctx = contexts[0];
  if (ctx) {
    await chrome.tabs.update(ctx.tabId, { active: true });
    await chrome.windows.update(ctx.windowId, { focused: true });
  } else {
    await chrome.tabs.create({ url: MANAGE });
  }
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
chrome.tabs.onUpdated.addListener((_id, change, tab) => { if (shouldRemember(change, tab)) void remember(tab); });

chrome.runtime.onMessage.addListener((message: unknown, sender, reply) => {
  const shaped = message as { type?: unknown } | null;
  if (shaped?.type !== "save-excerpt") return false;
  if (sender.id !== chrome.runtime.id || !isValidSaveExcerpt(message)) {
    reply({ ok: false });
    return true;
  }
  const m = message;
  void openDb()
    .then((db) => addExcerpt(db, { site: m.site, conversationId: m.conversationId, url: m.url, pageTitle: m.pageTitle, text: m.text.slice(0, 20_000), note: "" }, Date.now()))
    .then(() => reply({ ok: true }), (e) => reply({ ok: false, error: String(e) }));
  return true;
});
