import { describe, expect, it } from "vitest";
import { SiteError } from "../shared/types";
import type { Adapter } from "../sites/types";
import { createAgent } from "./agent";

const adapter: Adapter = {
  site: "claude", origin: "https://claude.ai", conversationUrl: (id) => id,
  account: async () => ({ remoteId: "org", label: "Claude" }),
  list: async () => { throw new SiteError("E_RATE", "", 3000); },
  read: async (id) => ({ id, title: "", updatedAt: 1, messages: [] }),
  remove: async () => {},
};

describe("content agent", () => {
  it("answers ops for its own site and serializes errors", async () => {
    const agent = createAgent(adapter);
    expect(await agent({ type: "site-rpc", site: "claude", op: "account", arg: null })).toEqual({ ok: true, value: { remoteId: "org", label: "Claude" } });
    expect(await agent({ type: "site-rpc", site: "claude", op: "list", arg: null })).toEqual({ ok: false, error: { code: "E_RATE", detail: "", retryAfterMs: 3000 } });
    expect(await agent({ type: "site-rpc", site: "claude", op: "archive", arg: "x" })).toMatchObject({ ok: false, error: { code: "E_HTTP" } });
    expect(await agent({ type: "site-rpc", site: "chatgpt", op: "account", arg: null })).toMatchObject({ ok: false, error: { code: "E_NO_AGENT" } });
  });
});
