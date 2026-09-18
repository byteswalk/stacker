import { describe, expect, it } from "vitest";
import { surfaceTaskText } from "./surfaceTask";
import type { AgentTask } from "../agent-tasks/taskStore";

const base: Omit<AgentTask, "state"> = {
  id: "t", productId: "pi", productName: "pi", surface: "cli", surfaceLabel: "pi", cliId: "pi", action: "update",
  message: null, lastLine: null, createdAt: "", startedAt: null, finishedAt: null,
};

describe("surface task text", () => {
  it("describes queued and running tasks only", () => {
    expect(surfaceTaskText(null)).toBeNull();
    expect(surfaceTaskText({ ...base, state: "queued" })).toBe("排队中");
    expect(surfaceTaskText({ ...base, state: "running", lastLine: "npm install" })).toBe("npm install");
    expect(surfaceTaskText({ ...base, state: "running" })).toBe("正在执行…");
    expect(surfaceTaskText({ ...base, state: "failed" })).toBeNull();
  });
});
