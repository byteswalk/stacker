import { describe, expect, it } from "vitest";
import type { EntryView } from "./api";
import { draftFrom, oversizedField, toEntryInput } from "./EntryEditor";

const saved: EntryView = {
  id: "e1", title: "Coding Plan", platform: "火山方舟", kind: "token_plan",
  fields: [
    { name: "Key", secret: true, value: null, filled: true },
    { name: "Base URL", secret: false, value: "https://ark", filled: true },
  ],
  expiresAt: "2026-10-31", tags: ["work", "ai"], note: "n", favorite: true,
  createdAt: 1, updatedAt: 2, deletedAt: null, historyCount: 0, ssh: null,
};

describe("entry editor drafts", () => {
  it("starts a new entry from the API Key template", () => {
    const draft = draftFrom(null);
    expect(draft.kind).toBe("api_key");
    expect(draft.fields.map((field) => [field.name, field.secret])).toEqual([["Key", true], ["Base URL", false]]);
  });

  it("keeps saved secrets unless a new value is typed", () => {
    const draft = draftFrom(saved);
    const input = toEntryInput(draft);
    expect(input.id).toBe("e1");
    expect(input.fields).toEqual([
      { name: "Key", previousName: "Key", value: null, secret: true },
      { name: "Base URL", previousName: "Base URL", value: "https://ark", secret: false },
    ]);
    expect(input.tags).toEqual(["work", "ai"]);
    expect(input.expiresAt).toBe("2026-10-31");
  });

  it("sends a renamed secret with its previous name, and new values when typed", () => {
    const draft = draftFrom(saved);
    draft.fields[0].name = "API Key";
    draft.fields[1].value = "https://new";
    draft.tags = " a, b ,, a ";
    draft.expiresAt = "";
    const input = toEntryInput(draft);
    expect(input.fields[0]).toEqual({ name: "API Key", previousName: "Key", value: null, secret: true });
    expect(input.fields[1].value).toBe("https://new");
    expect(input.tags).toEqual(["a", "b"]);
    expect(input.expiresAt).toBeNull();
  });

  it("keeps a saved secret's value when its secret flag is turned off and nothing is typed", () => {
    const draft = draftFrom(saved);
    draft.fields[0].secret = false;
    const input = toEntryInput(draft);
    expect(input.fields[0]).toEqual({ name: "Key", previousName: "Key", value: null, secret: false });
  });

  it("names the first field whose value is over 16 KB in bytes", () => {
    const draft = draftFrom(null);
    expect(oversizedField(draft)).toBeNull();
    draft.fields[0].value = "a".repeat(16 * 1024);
    expect(oversizedField(draft)).toBeNull();
    draft.fields[0].value = "a".repeat(16 * 1024 + 1);
    expect(oversizedField(draft)).toBe("Key");
    // 6000 CJK characters are 18000 bytes: under 16K characters but over 16 KB.
    draft.fields[0].value = "密".repeat(6000);
    expect(oversizedField(draft)).toBe("Key");
  });
});
