import { BridgeError, connectChrome, createBridge } from "./lib/bridge";
import { createBridgeHandler, isBridgeMessage } from "./lib/bridgeMessages";
import { addExcerpt, enqueueAll, onOutboxChange, openDb, outboxCount } from "./lib/db";
import { LAST_TAB_KEY, shouldRemember, siteTabOf } from "./lib/lastTab";
import { isValidSaveExcerpt } from "./lib/saveExcerpt";
import { flush } from "./lib/sync";
import { isTrustedSender } from "./lib/trustedSender";

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

// --- Stacker bridge: the background owns the native port; pages ask through messages. ---
const bridge = createBridge(connectChrome, {
  version: chrome.runtime.getManifest().version,
  onConnected: (hello) => {
    // Stacker's copy is empty (new install or wiped data): send everything once.
    void openDb()
      .then(async (db) => {
        if (hello.counts.conversations === 0 && (await db.count("conversations")) > 0) await enqueueAll(db, Date.now());
      })
      .catch((e) => console.warn("Stacker sync: queueing a full resync failed", e));
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
  } catch (e) {
    // Not connected or Stacker refused (BridgeError, e.g. E_NOT_CONNECTED/E_TIMEOUT): the changes
    // stay queued for the next try, silently. Anything else is unexpected — surface it.
    if (!(e instanceof BridgeError)) console.warn("Stacker sync: flush failed", e);
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

const EXTENSION_URL = chrome.runtime.getURL("");
const handleBridge = createBridgeHandler({ bridge, openDb, flushSoon, lastSyncAt: () => lastSyncAt });
chrome.runtime.onMessage.addListener((message: unknown, sender, reply) => {
  if (!isBridgeMessage(message)) return false;
  // Only the extension's own pages (manage, popup) may drive the bridge: a content script shares
  // the extension's sender.id but runs with the website's own sender.url, not the extension's.
  if (!isTrustedSender(sender, chrome.runtime.id, EXTENSION_URL)) {
    reply({ ok: false, error: "E_REQUEST" });
    return true;
  }
  void handleBridge(message).then(reply);
  return true;
});
