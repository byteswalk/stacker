import { describe, expect, it, vi } from "vitest";
import type { StackerCall } from "./bridgeMessages";
import { chunkText, pickSaver, stackerSaver, type SaveFn } from "./save";

type StackerFn = (call: StackerCall, payload: unknown) => Promise<unknown>;

describe("save", () => {
  it("never splits a surrogate pair", () => {
    const text = `${"a".repeat(9)}😀b`;
    const parts = chunkText(text, 10);
    expect(parts.join("")).toBe(text);
    expect(parts[0]).toBe("a".repeat(9));
    expect(chunkText("")).toEqual([""]);
  });

  it("writes the first piece, then appends to the path Stacker chose", async () => {
    const call = vi.fn<StackerFn>(async () => ({ path: "chatgpt/a (1).md", fullPath: "X" }));
    await stackerSaver(call, 4)("chatgpt/a.md", "abcdefghij", "text/markdown");
    expect(call.mock.calls).toEqual([
      ["saveExport", { path: "chatgpt/a.md", text: "abcd", append: false }],
      ["saveExport", { path: "chatgpt/a (1).md", text: "efgh", append: true }],
      ["saveExport", { path: "chatgpt/a (1).md", text: "ij", append: true }],
    ]);
  });

  it("uses Stacker when connected and the downloads folder otherwise", async () => {
    const download = vi.fn<SaveFn>(async () => {});
    const call = vi.fn<StackerFn>(async () => ({ path: "a.md" }));
    const offline = await pickSaver(async () => false, call, download);
    expect(offline.where).toBe("downloads");
    await offline.save("a.md", "x", "text/markdown");
    expect(download).toHaveBeenCalledWith("a.md", "x", "text/markdown");
    const online = await pickSaver(async () => true, call, download);
    expect(online.where).toBe("stacker");
    await online.save("a.md", "x", "text/markdown");
    expect(call).toHaveBeenCalledTimes(1);
  });
});
