// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../invoke";
import { SessionCatalog } from "./SessionCatalog";
import type { ProjectRow, Session } from "./types";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const session: Session = {
  id: "claude:s1", agent: "claude", nativeId: "s1", title: "桌面标题", titleSource: "client",
  project: { key: "d:\\repo", name: "repo", path: "D:\\repo", exists: true }, client: "desktop",
  createdAt: 1, updatedAt: 2, archived: false, pinned: false, status: "active",
  children: [{ id: "claude:s1:agent-1", kind: "subagent", title: "查找调用点", bytes: 1, path: "D:\\x" }],
  bytes: 10, path: "D:\\x.jsonl", inDesktopIndex: true, parentMissing: false, favorite: false, summary: null, summaryStale: false, summaryBy: "", summaryAt: 0, copies: [],
};

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers(); vi.clearAllMocks();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  vi.mocked(invoke).mockImplementation(async (command: string) => {
    if (command === "sessions_list") return { items: [session], total: 1, ids: [session.id], totalBytes: 10, warnings: [] };
    if (command === "sessions_projects") return [{ project: session.project, agents: ["claude"], sessions: 1, orphans: 0, bytes: 10, updatedAt: 2 }];
    if (command === "sessions_delete_preview") return { token: "t", mode: "slim_export", sessions: [], children: 0, files: 0, bytes: 0, blocked: [{ id: session.id, title: session.title, reason: "E_IN_DESKTOP" }], exportDir: "D:\\exports", created: 1 };
    return null;
  });
});

afterEach(() => { act(() => root.unmount()); host.remove(); vi.useRealTimers(); });

async function mount() {
  await act(async () => { root.render(<SessionCatalog onCleanup={() => {}} />); });
  await act(async () => { await vi.advanceTimersByTimeAsync(350); });
}

async function click(el: Element | null | undefined) {
  expect(el).toBeTruthy();
  await act(async () => { (el as HTMLElement).click(); });
}

const button = (text: string) => [...host.querySelectorAll("button")].find((b) => b.textContent?.includes(text));

describe("session catalog", () => {
  it("lists client titles and hides automation by default", async () => {
    await mount();
    expect(host.textContent).toContain("桌面标题");
    const call = vi.mocked(invoke).mock.calls.find(([c]) => c === "sessions_list");
    expect((call?.[1] as { query: { includeAutomation: boolean } }).query.includeAutomation).toBe(false);
  });

  it("shows subtasks without making them selectable", async () => {
    await mount();
    await click(button("子任务 1"));
    expect(host.textContent).toContain("查找调用点");
    expect(host.querySelectorAll(".session-children input").length).toBe(0);
  });

  it("previews deletion with slim export and shows blocked sessions", async () => {
    await mount();
    await click(host.querySelector(`input[aria-label='${session.title}']`));
    await click(button("删除…"));
    await act(async () => { await vi.advanceTimersByTimeAsync(10); });
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("sessions_delete_preview", { ids: [session.id], mode: "slim_export" });
    expect(host.textContent).toContain("请先在桌面端删除");
    expect(vi.mocked(invoke).mock.calls.map(([c]) => c)).not.toContain("sessions_delete_execute");
  });

  it("jumps from a project to its sessions", async () => {
    await mount();
    await click(button("项目"));
    await click(host.querySelector(".session-project:not(.head) .session-project-name"));
    await act(async () => { await vi.advanceTimersByTimeAsync(350); });
    const last = vi.mocked(invoke).mock.calls.filter(([c]) => c === "sessions_list").pop();
    expect((last?.[1] as { query: { project: string } }).query.project).toBe(session.project.key);
  });
});

describe("project filter", () => {
  it("only offers projects of the chosen agent and clears a project that no longer fits", async () => {
    const { SessionList } = await import("./SessionList");
    const onFilter = vi.fn();
    const projects: ProjectRow[] = [
      { project: { key: "a", name: "only-codex", path: "A", exists: true }, agents: ["codex"], sessions: 1, orphans: 0, discarded: 0, bytes: 1, updatedAt: 1 },
      { project: { key: "b", name: "only-claude", path: "B", exists: true }, agents: ["claude"], sessions: 1, orphans: 0, discarded: 0, bytes: 1, updatedAt: 1 },
    ];
    const query = { agent: "codex", project: "", status: "", client: "", search: "", fullText: false, includeAutomation: false, favoritesOnly: false, updatedAfter: 0, sort: "" as const, offset: 0 };
    await act(async () => { root.render(<SessionList page={{ items: [], total: 0, ids: [], totalBytes: 0, warnings: [], agents: [{ agent: "codex", sessions: 1, bytes: 1 }, { agent: "workbuddy", sessions: 4, bytes: 9 }] }} query={query} projects={projects} loading={false} selected={[]}
      onSelect={() => {}} onFilter={onFilter} onPage={() => {}} onOpen={() => {}} onFavorite={() => {}} onDelete={() => {}} onSummarize={() => {}} onDistill={() => {}} onMigrate={() => {}} />); });
    const selects = [...host.querySelectorAll("button")].filter((b) => b.textContent?.includes("全部项目"));
    await click(selects[0]);
    expect(host.textContent).toContain("only-codex");
    expect(host.textContent).not.toContain("only-claude");
  });
});

describe("status filter", () => {
  it("sends the chosen status to the backend", async () => {
    await mount();
    const trigger = [...host.querySelectorAll("button")].find((b) => b.textContent?.trim() === "全部状态");
    await click(trigger);
    const option = [...document.querySelectorAll("[role=option], button, div")].find((el) => el.textContent?.trim() === "未归档" && el !== trigger);
    await click(option);
    await act(async () => { await vi.advanceTimersByTimeAsync(350); });
    const last = vi.mocked(invoke).mock.calls.filter(([c]) => c === "sessions_list").pop();
    expect((last?.[1] as { query: { status: string } }).query.status).toBe("active");
  });
});
