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
