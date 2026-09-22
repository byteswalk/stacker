import { describe, expect, it } from "vitest";
import { venvSummary } from "./venvSummary";

const info = {
  project: "D:\\work\\app",
  dir: "D:\\work\\app\\.venv",
  python: "D:\\work\\app\\.venv\\Scripts\\python.exe",
  version: "3.12.9",
  base: "C:\\py\\3.12.9",
  baseExists: true,
};

describe("what an AI is told about a project venv", () => {
  it("points at the venv interpreter by absolute path", () => {
    const text = venvSummary(info);
    expect(text).toContain(`"${info.python}" -m pip install`);
    expect(text).toContain("不需要激活虚拟环境");
  });

  it("says a venv whose base Python is gone cannot be used", () => {
    const text = venvSummary({ ...info, baseExists: false });
    expect(text).toContain("已不存在");
    expect(text).not.toContain("-m pip install");
  });
});
