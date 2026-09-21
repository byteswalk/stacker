import { describe, expect, it } from "vitest";
import { surfaceTaskText } from "./surfaceTask";
import type { AgentTask } from "../agent-tasks/taskStore";

const base: Omit<AgentTask, "state"> = {
  id: "t", productId: "pi", productName: "pi", surface: "cli", surfaceLabel: "pi", cliId: "pi", action: "update",
  message: null, lastLine: null, waiting: null, createdAt: "", startedAt: null, finishedAt: null,
};

describe("surface task text", () => {
  it("describes queued and running tasks only", () => {
    expect(surfaceTaskText(null)).toBeNull();
    expect(surfaceTaskText({ ...base, state: "queued" })).toBe("排队中");
    expect(surfaceTaskText({ ...base, state: "queued", waiting: "等其他 npm 任务完成" })).toBe("排队中 · 等其他 npm 任务完成");
    expect(surfaceTaskText({ ...base, state: "running", lastLine: "npm install" })).toBe("npm install");
    expect(surfaceTaskText({ ...base, state: "running" })).toBe("正在执行…");
    // A bare "done / total" gets its share in front.
    expect(surfaceTaskText({ ...base, state: "running", lastLine: "24.0 MB / 48.0 MB" })).toBe("50% · 24.0 MB / 48.0 MB");
    // Stacker's own downloader already says its percentage; it is not repeated.
    expect(surfaceTaskText({ ...base, state: "running", lastLine: "正在下载 45% · 12.3/27.0 MB · 已 5s" })).toBe("正在下载 45% · 12.3/27.0 MB · 已 5s");
    expect(surfaceTaskText({ ...base, state: "failed" })).toBeNull();
  });
});
