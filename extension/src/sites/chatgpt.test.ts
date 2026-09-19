import { describe, expect, it, vi } from "vitest";
import type { FetchInit, FetchResult } from "./http";
import { chatgpt } from "./chatgpt";
import { conversation, imageConversation, listPage, session, sessionNoName, voiceConversation } from "./fixtures/chatgpt";

function fake(routes: Record<string, unknown>, calls: [string, FetchInit | undefined][] = []) {
  return vi.fn(async (url: string, init?: FetchInit): Promise<FetchResult> => {
    calls.push([url, init]);
    const key = Object.keys(routes).find((k) => url.includes(k));
    return key ? { status: 200, json: routes[key], retryAfter: null } : { status: 404, json: null, retryAfter: null };
  });
}

describe("chatgpt adapter", () => {
  it("names the account by user id and label, and never keeps the email", async () => {
    const a = chatgpt(fake({ "/api/auth/session": session }));
    const account = await a.account();
    expect(account).toEqual({ remoteId: "user-abc", label: "Ada Lovelace" });
    expect(JSON.stringify(account)).not.toContain("example.com");
  });

  it("falls back to the site name when the account has no name", async () => {
    const a = chatgpt(fake({ "/api/auth/session": sessionNoName }));
    expect(await a.account()).toEqual({ remoteId: "user-abc", label: "ChatGPT" });
  });

  it("lists live then archived pages with the bearer token", async () => {
    const calls: [string, FetchInit | undefined][] = [];
    const a = chatgpt(fake({ "/api/auth/session": session, "is_archived=true": { items: [], total: 0 }, "/backend-api/conversations": listPage }, calls));
    const first = await a.list(null);
    expect(first.items.map((i) => [i.id, i.archived])).toEqual([["c1", false], ["c2", false]]);
    expect(first.items[1].updatedAt).toBe(1_788_100_000_500);
    expect(first.next).toBe("archived:0");
    const second = await a.list(first.next);
    expect(second).toEqual({ items: [], next: null });
    const listCall = calls.find(([url]) => url.includes("/backend-api/conversations"))!;
    expect(listCall[1]?.headers?.Authorization).toBe("Bearer tok");
  });

  it("reads only the branch on screen, skipping hidden messages", async () => {
    const a = chatgpt(fake({ "/api/auth/session": session, "/backend-api/conversation/c1": conversation }));
    const body = await a.read("c1");
    expect(body.messages.map((m) => [m.role, m.text])).toEqual([["user", "Where to go?"], ["assistant", "Try Kyoto."], ["tool", "print(1)"]]);
    expect(body.messages[0].attachments).toEqual(["map.png"]);
  });

  it("turns a voice conversation's audio_transcription parts into text", async () => {
    const a = chatgpt(fake({ "/api/auth/session": session, "/backend-api/conversation/c1": voiceConversation }));
    const body = await a.read("c1");
    expect(body.messages.map((m) => [m.role, m.text])).toEqual([
      ["user", "Where should I go?"],
      ["assistant", "Try Kyoto."],
    ]);
    expect(body.messages[0].attachments).toEqual(["audio"]);
  });

  it("turns an image_asset_pointer part into an image attachment", async () => {
    const a = chatgpt(fake({ "/api/auth/session": session, "/backend-api/conversation/c1": imageConversation }));
    const body = await a.read("c1");
    expect(body.messages).toEqual([{ role: "user", text: "Look at this", at: 1_788_400_000_000, attachments: ["image"] }]);
  });

  it("deletes and archives with PATCH", async () => {
    const calls: [string, FetchInit | undefined][] = [];
    const a = chatgpt(fake({ "/api/auth/session": session, "/backend-api/conversation/c1": { success: true } }, calls));
    await a.remove("c1");
    await a.archive!("c1");
    const patches = calls.filter(([, init]) => init?.method === "PATCH").map(([, init]) => init?.body);
    expect(patches).toEqual([{ is_visible: false }, { is_archived: true }]);
  });

  it("treats a delete response without success: true as a changed interface", async () => {
    for (const reply of [{}, { success: false }, null, "ok"]) {
      const a = chatgpt(fake({ "/api/auth/session": session, "/backend-api/conversation/c1": reply }));
      await expect(a.remove("c1")).rejects.toThrow("E_BROKEN: remove response");
    }
  });

  it("keeps paging by page size when the list has no total", async () => {
    const full = { items: Array.from({ length: 100 }, (_, i) => ({ id: `x${i}`, title: "", create_time: 1, update_time: 1 })) };
    const a = chatgpt(fake({ "/api/auth/session": session, "offset=0&": full, "offset=100&": { items: [{ id: "y", title: "", create_time: 1, update_time: 1 }] } }));
    const first = await a.list(null);
    expect(first.next).toBe("live:100");
    expect((await a.list(first.next)).next).toBe("archived:0");
  });

  it("treats a missing token as signed out and a changed shape as broken", async () => {
    await expect(chatgpt(fake({ "/api/auth/session": {} })).account()).rejects.toThrow("E_AUTH");
    const a = chatgpt(fake({ "/api/auth/session": session, "/backend-api/conversations": { conversations: [] } }));
    await expect(a.list(null)).rejects.toThrow("E_BROKEN");
  });
});
