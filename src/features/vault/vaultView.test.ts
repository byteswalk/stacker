import { describe, expect, it } from "vitest";
import type { EntryView } from "./api";
import { daysUntil, expiryState, filterEntries, groupEntries, passwordStrength, platformsOf, siteKey, soonCount, systemOf } from "./vaultView";

const today = new Date(2026, 9, 1);

function entry(partial: Partial<EntryView>): EntryView {
  return {
    id: partial.title ?? "id", title: "t", platform: "", kind: "api_key", fields: [], expiresAt: null, tags: [], note: "",
    favorite: false, createdAt: 0, updatedAt: 0, deletedAt: null, historyCount: 0, windows: false, ssh: null, ...partial,
  };
}

describe("vault view helpers", () => {
  it("classifies expiry against a 14-day window", () => {
    expect(expiryState(null, today)).toBe("none");
    expect(expiryState("2026-09-30", today)).toBe("expired");
    expect(expiryState("2026-10-01", today)).toBe("soon");
    expect(expiryState("2026-10-15", today)).toBe("soon");
    expect(expiryState("2026-10-16", today)).toBe("ok");
    expect(daysUntil("2026-10-04", today)).toBe(3);
    expect(soonCount([entry({ expiresAt: "2026-10-02" }), entry({ expiresAt: "2027-01-01" }), entry({ expiresAt: "2026-09-01" })], today)).toBe(2);
  });

  it("searches visible text only and lists the latest change first", () => {
    const entries = [
      entry({ title: "Old", updatedAt: 1, note: "绑定 work 邮箱" }),
      entry({ title: "New", updatedAt: 2, platform: "GitHub" }),
      entry({ title: "Fav", updatedAt: 0, favorite: true, tags: ["ci"] }),
      entry({ title: "Masked", updatedAt: 3, fields: [{ name: "Key", secret: true, value: null, filled: true }, { name: "Base URL", secret: false, value: "https://ark.example", filled: true }] }),
    ];
    const all = { query: "", platform: "", soonOnly: false };
    expect(filterEntries(entries, all, today).map((e) => e.title)).toEqual(["Masked", "New", "Old", "Fav"]);
    expect(filterEntries(entries, { ...all, query: "WORK" }, today).map((e) => e.title)).toEqual(["Old"]);
    expect(filterEntries(entries, { ...all, query: "ark.example" }, today).map((e) => e.title)).toEqual(["Masked"]);
    expect(filterEntries(entries, { ...all, query: "ci" }, today).map((e) => e.title)).toEqual(["Fav"]);
    expect(filterEntries(entries, { ...all, platform: "GitHub" }, today).map((e) => e.title)).toEqual(["New"]);
    expect(platformsOf(entries)).toEqual(["GitHub"]);
  });

  it("filters to entries due soon", () => {
    const entries = [entry({ title: "a", expiresAt: "2026-10-05" }), entry({ title: "b" })];
    expect(filterEntries(entries, { query: "", platform: "", soonOnly: true }, today).map((e) => e.title)).toEqual(["a"]);
  });

  it("groups one site's logins and finds an account kept more than once", () => {
    const login = (id: string, url: string, user: string, updatedAt: number) => entry({
      title: id, updatedAt, tags: ["浏览器"],
      fields: [{ name: "网址", secret: false, value: url, filled: true }, { name: "账号", secret: false, value: user, filled: true }, { name: "密码", secret: true, value: null, filled: true }],
    });
    expect(siteKey(login("a", "https://accounts.google.com/x", "me", 1))).toBe("google.com");
    expect(siteKey(login("a", "https://www.taobao.com.cn/", "me", 1))).toBe("taobao.com.cn");
    expect(siteKey(login("a", "http://192.168.2.1:8080/", "me", 1))).toBe("192.168.2.1");
    expect(siteKey(entry({ title: "My Token", platform: "GitHub" }))).toBe("github");

    const groups = groupEntries([
      login("g1", "https://mail.google.com/", "me", 5),
      login("x", "https://example.com/", "ann", 4),
      login("g2", "https://mail.google.com", "me", 3),
      login("g3", "https://google.com/", "you", 2),
    ]);
    expect(groups.map((group) => [group.key, group.entries.map((e) => e.title)])).toEqual([["google.com", ["g1", "g2", "g3"]], ["example.com", ["x"]]]);
    expect(groups[0].duplicates.map((set) => set.map((e) => e.title))).toEqual([["g1", "g2"]]);
  });

  it("takes only logins at the same address for duplicates", () => {
    const login = (id: string, url: string) => entry({
      title: id, updatedAt: 1,
      fields: [{ name: "网址", secret: false, value: url, filled: true }, { name: "账号", secret: false, value: "admin", filled: true }],
    });
    const router = login("router", "https://192.168.2.1/userLogin.asp");
    expect(systemOf(router)).toBe(systemOf(login("x", "HTTPS://192.168.2.1:443/userLogin.asp#top")));
    expect(systemOf(login("x", "https://192.168.2.1"))).toBe(systemOf(login("x", "https://192.168.2.1/")));
    expect(systemOf(router)).not.toBe(systemOf(login("x", "https://192.168.2.1/")));
    expect(systemOf(router)).not.toBe(systemOf(login("x", "http://192.168.2.1:1188/")));
    expect(systemOf(login("x", "https://h/login?app=1"))).not.toBe(systemOf(login("x", "https://h/login?app=2")));
    const groups = groupEntries([router, login("root", "https://192.168.2.1/"), login("nas", "http://192.168.2.1:1188/"),
      login("again", "https://192.168.2.1/userLogin.asp"), login("web", "http://192.168.2.1:6086/")]);
    expect(groups).toHaveLength(1);
    expect(groups[0].duplicates.map((set) => set.map((e) => e.title))).toEqual([["router", "again"]]);
  });

  it("filters by kind, source and Windows credentials", () => {
    const entries = [
      entry({ title: "ssh", kind: "ssh_key" }),
      entry({ title: "web", tags: ["浏览器"] }),
      entry({ title: "token", windows: true }),
    ];
    const all = { query: "", platform: "", soonOnly: false };
    expect(filterEntries(entries, { ...all, kind: "ssh_key" }, today).map((e) => e.title)).toEqual(["ssh"]);
    expect(filterEntries(entries, { ...all, source: "browser" }, today).map((e) => e.title)).toEqual(["web"]);
    expect(filterEntries(entries, { ...all, source: "own" }, today).map((e) => e.title)).toEqual(["ssh", "token"]);
    expect(filterEntries(entries, { ...all, windows: "on" }, today).map((e) => e.title)).toEqual(["token"]);
  });

  it("rates passwords by length and character classes", () => {
    expect(passwordStrength("")).toBe(0);
    expect(passwordStrength("short")).toBe(1);
    expect(passwordStrength("12345678")).toBe(1);
    expect(passwordStrength("abcdefghi")).toBe(2);
    expect(passwordStrength("abcdefgh1")).toBe(2);
    expect(passwordStrength("Abcdefg1!")).toBe(3);
    expect(passwordStrength("abcdefghijklmn")).toBe(3);
    expect(passwordStrength("Abcdefghij1!xy")).toBe(4);
  });
});
