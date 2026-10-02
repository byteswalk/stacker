import { t } from "../i18n";
import { captureFrom, fillField, passwordFields, userFieldFor } from "./detect";
import type { LoginChoice, LoginMessage, PageAnswer } from "./messages";

const ORANGE = "#f5821f";
const PANEL = "position:fixed;z-index:2147483647;font:13px/1.4 system-ui,'Microsoft YaHei',sans-serif;color:#e7e9ec;background:#1b2029;border:1px solid rgba(255,255,255,.14);border-radius:10px;box-shadow:0 8px 28px rgba(0,0,0,.35);";
const BUTTON = "border:1px solid rgba(255,255,255,.18);background:transparent;color:inherit;border-radius:7px;padding:5px 10px;font:inherit;font-size:12px;cursor:pointer;";

function ask(message: LoginMessage): Promise<unknown> {
  return chrome.runtime.sendMessage(message).catch(() => null);
}

/** Everything the page shows lives in one closed shadow root; the site's own page is left alone. */
function makeRoot(doc: Document): ShadowRoot {
  const host = doc.createElement("stacker-logins");
  const root = host.attachShadow({ mode: "closed" });
  doc.documentElement.append(host);
  return root;
}

function button(doc: Document, label: string, primary = false): HTMLButtonElement {
  const el = doc.createElement("button");
  el.textContent = label;
  el.setAttribute("style", BUTTON + (primary ? `background:${ORANGE};border-color:${ORANGE};color:#fff;` : ""));
  el.addEventListener("mousedown", (e) => e.preventDefault());
  return el;
}

/** The bar that asks whether to keep a login just submitted. */
function showSaveBar(doc: Document, root: ShadowRoot, pending: { user: string; host: string }) {
  root.querySelector("[data-bar]")?.remove();
  const bar = doc.createElement("div");
  bar.dataset.bar = "";
  bar.setAttribute("style", PANEL + "top:14px;right:14px;width:340px;padding:12px 14px;display:flex;flex-direction:column;gap:10px;");
  const title = doc.createElement("div");
  title.setAttribute("style", "font-weight:600;");
  title.textContent = t("保存到 Stacker 密钥保管？");
  const line = doc.createElement("div");
  line.setAttribute("style", "color:#9aa2ad;font-size:12px;word-break:break-all;");
  line.textContent = `${pending.host}${pending.user ? ` · ${pending.user}` : ""}`;
  const row = doc.createElement("div");
  row.setAttribute("style", "display:flex;gap:6px;flex-wrap:wrap;");
  const choices: [string, "save" | "save-fill" | "never" | "dismiss", boolean][] = [
    [t("保存"), "save", false],
    [t("保存并允许填充"), "save-fill", true],
    [t("不保存"), "dismiss", false],
    [t("此网站不再询问"), "never", false],
  ];
  for (const [label, choice, primary] of choices) {
    const el = button(doc, label, primary);
    el.addEventListener("click", async () => {
      const result = (await ask({ type: "logins-decide", choice })) as { ok?: boolean } | null;
      if (choice === "save" || choice === "save-fill") {
        line.textContent = result?.ok ? t("已交给 Stacker，解锁保管库后收进去。") : t("没能交给 Stacker：请确认 Stacker 已连接这个浏览器。");
        row.remove();
        setTimeout(() => bar.remove(), 4000);
      } else {
        bar.remove();
      }
    });
    row.append(el);
  }
  const note = doc.createElement("div");
  note.setAttribute("style", "color:#9aa2ad;font-size:11px;");
  note.textContent = t("「允许填充」的登录会放进 Windows 凭据管理器，以后在这个网站点一下就能填。");
  bar.append(title, line, row, note);
  root.append(bar);
}

/** The short list under a login field: which account to fill. */
function attachFill(doc: Document, root: ShadowRoot, logins: LoginChoice[]) {
  let list: HTMLDivElement | null = null;
  const hide = () => { list?.remove(); list = null; };
  const show = (field: HTMLInputElement, password: HTMLInputElement) => {
    hide();
    const rect = field.getBoundingClientRect();
    list = doc.createElement("div");
    list.setAttribute("style", PANEL + `left:${Math.max(rect.left, 6)}px;top:${rect.bottom + 4}px;min-width:${Math.max(rect.width, 220)}px;padding:6px;`);
    const head = doc.createElement("div");
    head.setAttribute("style", "color:#9aa2ad;font-size:11px;padding:2px 6px 6px;");
    head.textContent = t("用 Stacker 填入");
    list.append(head);
    for (const login of logins.slice(0, 6)) {
      const item = button(doc, login.user || login.title);
      item.setAttribute("style", BUTTON + "display:block;width:100%;text-align:left;border:0;padding:6px 8px;");
      item.title = login.title;
      item.addEventListener("click", async () => {
        const result = (await ask({ type: "logins-fill", user: login.user })) as { ok?: boolean; password?: string } | null;
        hide();
        if (!result?.ok || typeof result.password !== "string") return;
        const user = userFieldFor(password);
        if (user && login.user) fillField(user, login.user);
        fillField(password, result.password);
      });
      list.append(item);
    }
    root.append(list);
  };
  doc.addEventListener("focusin", (event) => {
    const field = event.target;
    if (!(field instanceof HTMLInputElement)) return;
    const password = field.type === "password" ? field : passwordFields(field.form ?? doc)[0];
    if (!password || (field !== password && userFieldFor(password) !== field)) return;
    show(field, password);
  });
  doc.addEventListener("focusout", () => setTimeout(hide, 150));
  doc.addEventListener("scroll", hide, true);
}

/** Watches for a login being submitted and passes it to the background, which decides whether to ask. */
function watchSubmits(doc: Document, root: ShadowRoot) {
  let last = "";
  const report = async (scope: ParentNode) => {
    const found = captureFrom(scope);
    if (!found) return;
    const key = `${found.user}\n${found.password}`;
    if (key === last) return;
    last = key;
    const result = (await ask({ type: "logins-captured", user: found.user, password: found.password, title: doc.title })) as { prompt?: boolean } | null;
    // Pages that stay put after signing in get the question here; the others on the next page.
    if (result?.prompt) showSaveBar(doc, root, { user: found.user, host: location.hostname.replace(/^www\./, "") });
  };
  doc.addEventListener("submit", (event) => { if (event.target instanceof HTMLFormElement) void report(event.target); }, true);
  doc.addEventListener("click", (event) => {
    const target = event.target instanceof Element ? event.target.closest("button, input[type=submit], [role=button]") : null;
    if (!target) return;
    const form = target.closest("form");
    if (form ? passwordFields(form).length : passwordFields(doc).length) void report(form ?? doc);
  }, true);
  doc.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && event.target instanceof HTMLInputElement && event.target.type === "password") void report(event.target.form ?? doc);
  }, true);
}

export async function installLogins(doc: Document = document): Promise<void> {
  const root = makeRoot(doc);
  watchSubmits(doc, root);
  const answer = (await ask({ type: "logins-page" })) as PageAnswer | null;
  if (answer?.pending) showSaveBar(doc, root, answer.pending);
  if (answer?.logins?.length) attachFill(doc, root, answer.logins);
}
