import { describe, expect, it } from "vitest";
import type { Conversation } from "./db";
import { BROKEN_SITES_KEY, brokenSitesIn, createBrokenSites, type SessionArea } from "./brokenSites";

function fakeArea(): SessionArea & { data: Record<string, unknown> } {
  const data: Record<string, unknown> = {};
  return { data, get: async (key) => (key in data ? { [key]: data[key] } : {}), set: async (items) => { Object.assign(data, items); } };
}
const conv = (site: "chatgpt" | "claude", id: string) => ({ key: `${site}:${id}`, site }) as Conversation;

describe("broken sites", () => {
  it("remembers a broken site until it is cleared", async () => {
    const area = fakeArea();
    const broken = createBrokenSites(area);
    expect(await broken.all()).toEqual({});
    await broken.mark(["chatgpt"], 5);
    expect(area.data[BROKEN_SITES_KEY]).toEqual({ chatgpt: 5 });
    expect(await createBrokenSites(area).all()).toEqual({ chatgpt: 5 });
    await broken.mark(["claude"], 6);
    expect(await broken.clear("chatgpt")).toEqual({ claude: 6 });
    expect(await broken.all()).toEqual({ claude: 6 });
  });
  it("finds the sites whose items failed with E_BROKEN, not those merely skipped", () => {
    const items = [conv("chatgpt", "a"), conv("claude", "b"), conv("claude", "c")];
    expect(brokenSitesIn(items, [
      { key: "chatgpt:a", title: "", status: "failed", error: "E_BROKEN" },
      { key: "claude:b", title: "", status: "skipped", error: "E_BROKEN" },
      { key: "claude:c", title: "", status: "failed", error: "E_NET" },
    ])).toEqual(["chatgpt"]);
  });
});
