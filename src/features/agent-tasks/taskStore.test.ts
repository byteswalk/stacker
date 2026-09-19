import { describe, expect, it } from "vitest";
import { applyTaskEvent, openTaskFor, type AgentTask, type TaskMap } from "./taskStore";

function task(overrides: Partial<AgentTask>): AgentTask {
  return {
    id: "t1", productId: "workbuddy-cn", productName: "WorkBuddy 中国版", surface: "cli", surfaceLabel: "CodeBuddy CLI",
    cliId: "codebuddy", action: "update", state: "queued", message: null, lastLine: null,
    createdAt: "2026-09-18T00:00:00Z", startedAt: null, finishedAt: null, ...overrides,
  };
}

describe("agent task store", () => {
  it("reports a finished task exactly once", () => {
    let result = applyTaskEvent({}, task({ state: "running" }));
    expect(result.finished).toBeNull();
    result = applyTaskEvent(result.tasks, task({ state: "succeeded" }));
    expect(result.finished?.id).toBe("t1");
    result = applyTaskEvent(result.tasks, task({ state: "succeeded" }));
    expect(result.finished).toBeNull();
  });

  it("finds the open task of a shared CLI from either product card", () => {
    const tasks: TaskMap = { t1: task({ state: "running" }) };
    expect(openTaskFor(tasks, "workbuddy-global", "codebuddy", "cli")?.id).toBe("t1");
    expect(openTaskFor(tasks, "workbuddy-global", "codebuddy", "desktop")).toBeNull();
    expect(openTaskFor({ t1: task({ state: "failed" }) }, "workbuddy-cn", "codebuddy", "cli")).toBeNull();
  });
});
