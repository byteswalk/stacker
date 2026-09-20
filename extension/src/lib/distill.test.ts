import { describe, expect, it, vi } from "vitest";
import { distillResults } from "./distill";

describe("distilled results", () => {
  it("asks Stacker for one conversation's results", async () => {
    const send = vi.fn(async () => ({
      ok: true,
      value: { items: [{ id: "qa-1", kind: "qa", title: "Where to go", body: "Kyoto.", state: "draft", updatedAt: 2, sources: ["Trip plan"] }] },
    }));
    const items = await distillResults("chatgpt", "a", send);
    expect(send).toHaveBeenCalledWith({ type: "bridge-call", call: "distillResults", payload: { site: "chatgpt", id: "a" } });
    expect(items).toHaveLength(1);
    expect(items[0].title).toBe("Where to go");
  });

  it("is empty when Stacker answers with nothing usable", async () => {
    const send = vi.fn(async () => ({ ok: true, value: {} }));
    expect(await distillResults("chatgpt", "a", send)).toEqual([]);
  });
});
