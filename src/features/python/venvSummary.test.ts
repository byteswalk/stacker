import { describe, expect, it } from "vitest";
import { venvSummary, type VenvInfo } from "./venvSummary";

const info: VenvInfo = {
  project: "D:\\work\\app",
  kind: "venv",
  dir: "D:\\work\\app\\.venv",
  python: "D:\\work\\app\\.venv\\Scripts\\python.exe",
  version: "3.12.9",
  base: "C:\\py\\3.12.9",
  baseExists: true,
  pipIni: null,
};

describe("what an AI is told about a project environment", () => {
  it("points at the venv interpreter by absolute path", () => {
    const text = venvSummary(info);
    expect(text).toContain(`"${info.python}" -m pip install`);
    expect(text).toContain("不需要激活虚拟环境");
    expect(text).toContain("虚拟环境，由 Stacker 管理");
  });

  it("says a venv whose base Python is gone cannot be used", () => {
    const text = venvSummary({ ...info, baseExists: false });
    expect(text).toContain("已不存在");
    expect(text).not.toContain("-m pip install");
  });

  it("describes an embedded runtime as the project's own", () => {
    const text = venvSummary({
      ...info,
      kind: "embedded",
      dir: "E:\\app\\runtime\\faster-whisper",
      python: "E:\\app\\runtime\\faster-whisper\\python.exe",
      base: null,
      pipIni: "E:\\app\\runtime\\faster-whisper\\pip.ini",
    });
    expect(text).toContain("项目自带运行时");
    expect(text).toContain("不要把它当成系统 Python");
    expect(text).toContain("优先级高于用户级 pip.ini");
    expect(text).not.toContain("创建自");
  });
});
