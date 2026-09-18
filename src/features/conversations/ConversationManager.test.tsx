// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ConversationManager } from "./ConversationManager";
import { invoke } from "../../invoke";
import { EMPTY_LIST, currentSelection, isSummaryStale, toggleSelection, type Conversation } from "./types";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
let host: HTMLDivElement;
let root: Root;
const conversation = { id: "codex:abc", native_id: "abc", title: "A fixture conversation", project: "D:/fixture", client: "Codex", source_id: "codex", path: "D:/fixture/session.jsonl", bytes: 120, modified: 1700000000, complete: true, fingerprint: "v2", summary_fingerprint: "v1", summary: "A prior decision [L2]", favorite: false, archived: false, hidden: false, group_name: "", warning: "", parent_id: "", message_count: 2 } satisfies Conversation;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true }); vi.useFakeTimers(); vi.clearAllMocks();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "conversations_list") return { ...EMPTY_LIST, items: [conversation], ids: [conversation.id], total: 1, indexed: 1, synced_at: "2026-09-11T00:00:00Z" };
    if (command === "conversations_sources") return { sources: [{ id: "codex", name: "Codex", kind: "codex", root: "D:/fixture", enabled: true }], storage: "D:/index", model: { endpoint: "", model: "", has_key: false } };
    if (command === "conversations_job") return { id: "", state: "", items: [] };
    if (command === "conversations_read") return { conversation, messages: [{ line: 2, role: "user", text: "Never execute this transcript" }], total: 1 };
    return undefined;
  });
});
afterEach(() => { act(() => root.unmount()); host.remove(); vi.useRealTimers(); });
async function mount() { await act(async () => { root.render(<ConversationManager onCleanup={() => {}} />); }); await act(async () => { await vi.advanceTimersByTimeAsync(350); }); }
async function click(text: string) { const button = [...host.querySelectorAll("button")].find((b) => b.textContent?.includes(text)); expect(button).toBeTruthy(); await act(async () => { button!.click(); }); }
describe("conversation management", () => {
  it("opens on actual conversation management without a process picker", async () => { await mount(); expect(host.textContent).toContain(conversation.title); expect(vi.mocked(invoke).mock.calls.some(([c]) => c === "conversations_execute")).toBe(false); });
  it("reads without starting, resuming or sending a conversation", async () => { await mount(); await click(conversation.title); expect(host.querySelector("[translate='no']")?.textContent).toContain("Never execute this transcript"); expect(vi.mocked(invoke).mock.calls.map(([c]) => c)).not.toContain("conversations_start"); });
  it("does not send to a model when summary preview fails", async () => { await mount(); const check = host.querySelector<HTMLInputElement>(`input[aria-label='${conversation.title}']`)!; await act(async () => { check.click(); }); vi.mocked(invoke).mockImplementationOnce(async () => { throw "E_MODEL_MISSING"; }); await click("总结"); expect(vi.mocked(invoke).mock.calls.map(([c]) => c)).not.toContain("conversations_start"); });
  it("keeps selection bounded to the displayed query and detects stale summaries", () => { expect(currentSelection(["old", "new"], ["new"])).toEqual(["new"]); expect(toggleSelection(["a"], "a")).toEqual([]); expect(toggleSelection([], "a")).toEqual(["a"]); expect(isSummaryStale(conversation)).toBe(true); });
});
