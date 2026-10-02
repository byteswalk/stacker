// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { captureFrom, fillField, userFieldFor } from "./detect";
import { createLoginHandler, type LoginDeps } from "./background";
import { isLoginMessage, siteOf } from "./messages";

// jsdom lays nothing out: every field counts as shown.
beforeEach(() => {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({ width: 100, height: 20, top: 0, left: 0, bottom: 20, right: 100, x: 0, y: 0, toJSON: () => ({}) });
});

function page(html: string): Document {
  document.body.innerHTML = html;
  return document;
}

describe("finding a login on a page", () => {
  it("takes the account field before the password, and prefers one marked as the username", () => {
    const doc = page(`<form><input name="q"><input name="email" value="me@x.com"><input type="password" value="pw"></form>`);
    const password = doc.querySelector<HTMLInputElement>("input[type=password]")!;
    expect(userFieldFor(password)?.name).toBe("email");
    expect(captureFrom(doc.forms[0])).toEqual({ user: "me@x.com", password: "pw" });
  });

  it("keeps the new password when a form changes it, and nothing when no password was typed", () => {
    const doc = page(`<form><input name="user" value="me"><input type="password" value="old"><input type="password" autocomplete="new-password" value="new"><input type="password" value="new"></form>`);
    expect(captureFrom(doc.forms[0])).toEqual({ user: "me", password: "new" });
    expect(captureFrom(page(`<form><input name="user" value="me"><input type="password"></form>`).forms[0])).toBeNull();
  });

  it("fills a field so that the page hears it", () => {
    const doc = page(`<input id="f">`);
    const input = doc.querySelector<HTMLInputElement>("#f")!;
    const heard = vi.fn();
    input.addEventListener("input", heard);
    fillField(input, "secret");
    expect(input.value).toBe("secret");
    expect(heard).toHaveBeenCalled();
  });
});

describe("messages from the page", () => {
  it("are shape-checked, and only http(s) pages have a site", () => {
    expect(isLoginMessage({ type: "logins-captured", user: "me", password: "pw", title: "" })).toBe(true);
    expect(isLoginMessage({ type: "logins-captured", user: "me", password: "", title: "" })).toBe(false);
    expect(isLoginMessage({ type: "logins-decide", choice: "steal" })).toBe(false);
    expect(isLoginMessage({ type: "other" })).toBe(false);
    expect(siteOf("https://www.GitHub.com/login")).toBe("github.com");
    expect(siteOf("chrome://settings")).toBeNull();
  });
});

function store() {
  const map = new Map<string, unknown>();
  return {
    map,
    get: async (key: string) => map.get(key),
    set: async (key: string, value: unknown) => { map.set(key, value); },
    remove: async (key: string) => { map.delete(key); },
  };
}

function deps(calls: Record<string, (payload: Record<string, unknown>) => unknown>) {
  return {
    call: vi.fn(async (type: string, payload: unknown) => {
      const fn = calls[type];
      if (!fn) throw new Error("E_NOT_CONNECTED");
      return fn(payload as Record<string, unknown>);
    }),
    session: store(),
    local: store(),
    now: () => 1000,
  } satisfies LoginDeps;
}

const ID = "ext";
const sender = (url: string) => ({ id: ID, url, tab: { id: 7 } });

describe("the background's half", () => {
  it("asks about a new login, saves it with the page's own address, and forgets it after", async () => {
    const d = deps({ loginsFor: () => ({ logins: [] }), loginSave: (p) => ({ saved: true, p }) });
    const handle = createLoginHandler(d, ID);
    expect(await handle({ type: "logins-captured", user: "me", password: "pw", title: "GitHub" }, sender("https://github.com/session"))).toEqual({ prompt: true });
    // The next page on the same site picks the question up; another site does not.
    expect(await handle({ type: "logins-page" }, sender("https://github.com/"))).toEqual({ pending: { user: "me", host: "github.com" }, logins: [] });
    expect((await handle({ type: "logins-page" }, sender("https://evil.com/")) as { pending: unknown }).pending).toBeNull();
    expect(await handle({ type: "logins-decide", choice: "save-fill" }, sender("https://github.com/"))).toEqual({ ok: true });
    expect(d.call).toHaveBeenCalledWith("loginSave", { url: "https://github.com/session", user: "me", password: "pw", title: "GitHub", fill: true });
    expect(d.session.map.size).toBe(0);
  });

  it("does not ask about a login already kept with that password, nor on a site told never", async () => {
    const d = deps({ loginsFor: () => ({ logins: [{ user: "me", title: "GitHub", host: "github.com" }] }), loginPassword: () => ({ password: "pw" }) });
    const handle = createLoginHandler(d, ID);
    expect(await handle({ type: "logins-captured", user: "me", password: "pw", title: "" }, sender("https://github.com/"))).toEqual({ prompt: false });
    expect(await handle({ type: "logins-captured", user: "me", password: "changed", title: "" }, sender("https://github.com/"))).toEqual({ prompt: true });
    await handle({ type: "logins-decide", choice: "never" }, sender("https://github.com/"));
    expect(await handle({ type: "logins-captured", user: "me", password: "other", title: "" }, sender("https://github.com/"))).toEqual({ prompt: false });
  });

  it("fills only through the browser's record of the page, and refuses anyone else", async () => {
    const d = deps({ loginPassword: (p) => ({ password: p.url === "https://github.com/login" ? "pw" : "" }) });
    const handle = createLoginHandler(d, ID);
    expect(await handle({ type: "logins-fill", user: "me" }, sender("https://github.com/login"))).toEqual({ ok: true, password: "pw" });
    expect(await handle({ type: "logins-fill", user: "me" }, { id: "another-extension", url: "https://github.com/login", tab: { id: 1 } })).toEqual({ ok: false, error: "E_REQUEST" });
    expect(await handle({ type: "logins-fill", user: "me" }, { id: ID, url: "chrome://settings", tab: { id: 1 } })).toEqual({ ok: false, error: "E_REQUEST" });
  });

  it("offers nothing when Stacker is not connected", async () => {
    const handle = createLoginHandler(deps({}), ID);
    expect(await handle({ type: "logins-page" }, sender("https://github.com/"))).toEqual({ pending: null, logins: [] });
  });
});
