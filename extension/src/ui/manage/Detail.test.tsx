// @vitest-environment jsdom
Object.defineProperty(navigator, "language", { value: "zh-CN", configurable: true });
import "fake-indexeddb/auto";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { openDb, type Conversation, type Db } from "../../lib/db";
import { Detail } from "./Detail";

vi.mock("../../lib/distill", async () => {
  const actual = await vi.importActual<typeof import("../../lib/distill")>("../../lib/distill");
  return { ...actual, distillResults: vi.fn() };
});
const { distillResults } = await import("../../lib/distill");

const conv: Conversation = {
  key: "chatgpt:a", site: "chatgpt", account: "chatgpt:u1", id: "a", title: "Trip plan",
  createdAt: 1, updatedAt: 2, archived: false, bodyFetchedAt: null, bodyUpdatedAt: null,
  removedAt: null, listedAt: 1, localUpdatedAt: 1, folderId: null, tags: [], favorite: false, note: "",
};

let host: HTMLDivElement;
let root: Root;
let db: Db;

beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  db = await openDb("detail-distill");
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

async function mount() {
  await act(async () => {
    root.render(<Detail db={db} conv={conv} folders={[]} reading={false} onRead={() => {}} onChanged={() => {}} />);
  });
  await act(async () => { await Promise.resolve(); });
}

describe("conversation detail", () => {
  it("lists the conversation's distilled results, read only", async () => {
    vi.mocked(distillResults).mockResolvedValue([
      { id: "qa-1", kind: "qa", title: "Where to go", body: "Kyoto.", state: "adopted", updatedAt: 2, sources: ["Trip plan"] },
    ]);
    await mount();
    expect(distillResults).toHaveBeenCalledWith("chatgpt", "a");
    // The results live behind their own tab; opening it is the only way in, and there is no
    // control anywhere that would start a distillation from here.
    await act(async () => [...host.querySelectorAll<HTMLElement>("[role=tab]")].find((x) => x.textContent?.includes("提炼结果"))!.click());
    expect(host.textContent).toContain("Where to go");
    expect(host.textContent).toContain("Kyoto.");
    expect([...host.querySelectorAll("button")].some((b) => b.textContent?.includes("提炼"))).toBe(false);
  });

  it("shows nothing when Stacker is not connected", async () => {
    vi.mocked(distillResults).mockRejectedValue(new Error("E_NOT_CONNECTED"));
    await mount();
    expect(host.textContent).not.toContain("提炼结果");
  });
});
