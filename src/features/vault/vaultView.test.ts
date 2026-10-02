import { describe, expect, it } from "vitest";
import type { EntryView } from "./api";
import { daysUntil, expiryState, filterEntries, passwordStrength, platformsOf, soonCount } from "./vaultView";

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
