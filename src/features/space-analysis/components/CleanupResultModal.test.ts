import { describe, expect, it } from "vitest";
import { cleanupLabel } from "./CleanupResultModal";

describe("cleanup result labels", () => {
  it("says in words why an item was not deleted", () => {
    expect(cleanupLabel("spaceAnalysis.cleanup.reason.accessDenied")).toBe("拒绝访问");
    expect(cleanupLabel("spaceAnalysis.cleanup.reason.elevationFailed")).toBe("管理员权限未获批准，或提权助手未完成");
  });

  it("names the states a task and its items can be in", () => {
    expect(cleanupLabel("completed")).toBe("已完成");
    expect(cleanupLabel("cancelling")).toBe("正在取消");
  });

  it("shows an unknown key as it came, so a gap is visible", () => {
    expect(cleanupLabel("spaceAnalysis.cleanup.reason.somethingNew")).toBe("spaceAnalysis.cleanup.reason.somethingNew");
  });
});
