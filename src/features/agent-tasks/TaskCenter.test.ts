import { describe, expect, it, vi } from "vitest";

vi.mock("../../invoke", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
const { invoke } = await import("../../invoke");
const { formatDuration, splitTasks } = await import("./TaskCenter");
const { clearFinishedTasks, dismissTask, initAgentTasks, subscribeTasks, taskSnapshot } = await import("./taskStore");
type AgentTask = import("./taskStore").AgentTask;

function task(id: string, state: AgentTask["state"], at: { created: string; finished?: string }): AgentTask {
  return {
    id, productId: id, productName: id, surface: "cli", surfaceLabel: id, cliId: null, action: "update", state,
    message: null, lastLine: null, waiting: null, createdAt: at.created, startedAt: null, finishedAt: at.finished ?? null,
  };
}

describe("task center", () => {
  it("says how long in the largest whole unit", () => {
    expect(formatDuration(42)).toBe("42 秒");
    expect(formatDuration(59.6)).toBe("1 分钟");
    expect(formatDuration(125)).toBe("2 分钟");
    expect(formatDuration(7300)).toBe("2 小时");
    expect(formatDuration(-3)).toBe("0 秒");
  });

  it("lists running tasks first, queued ones in the order they will start, finished ones newest first", () => {
    const { open, done } = splitTasks([
      task("q2", "queued", { created: "2026-09-21T10:00:02Z" }),
      task("old", "succeeded", { created: "2026-09-21T09:00:00Z", finished: "2026-09-21T09:01:00Z" }),
      task("q1", "queued", { created: "2026-09-21T10:00:01Z" }),
      task("run", "running", { created: "2026-09-21T10:00:03Z" }),
      task("new", "failed", { created: "2026-09-21T09:30:00Z", finished: "2026-09-21T09:40:00Z" }),
    ]);
    expect(open.map((t) => t.id)).toEqual(["run", "q1", "q2"]);
    expect(done.map((t) => t.id)).toEqual(["new", "old"]);
  });

  it("clears finished tasks down to what the backend still has, and dismisses one", async () => {
    const running = task("run", "running", { created: "2026-09-21T10:00:00Z" });
    const finished = task("done", "succeeded", { created: "2026-09-21T09:00:00Z", finished: "2026-09-21T09:01:00Z" });
    const other = task("other", "failed", { created: "2026-09-21T09:00:00Z", finished: "2026-09-21T09:02:00Z" });
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "agent_tasks") return [running, finished, other];
      if (command === "agent_tasks_clear") return [running];
      return undefined;
    });
    await initAgentTasks(() => {});
    const seen: string[][] = [];
    const stop = subscribeTasks((map) => seen.push(Object.keys(map).sort()));

    await dismissTask("other");
    expect(invoke).toHaveBeenCalledWith("agent_task_dismiss", { id: "other" });
    expect(Object.keys(taskSnapshot()).sort()).toEqual(["done", "run"]);

    await clearFinishedTasks();
    expect(Object.keys(taskSnapshot())).toEqual(["run"]);
    expect(seen).toEqual([["done", "run"], ["run"]]);
    stop();
  });
});
