import { describe, expect, it } from "vitest";
import { pythonSummary, type PythonEnvReport } from "./pythonSummary";

const root = "C:\\Users\\me\\.pyenv\\pyenv-win\\";
const report = (patch: Partial<PythonEnvReport> = {}): PythonEnvReport => ({
  pyenvRoot: root,
  pyenvBin: root + "bin",
  pyenvShims: root + "shims",
  defaultVersion: "3.13.14",
  defaultDir: root + "versions\\3.13.14",
  defaultPython: root + "versions\\3.13.14\\python.exe",
  defaultScripts: root + "versions\\3.13.14\\Scripts",
  pathPythons: [
    { dir: root + "versions\\3.13.14", program: "python.exe", scope: "user", kind: "default" },
    { dir: "C:\\Users\\me\\AppData\\Local\\Microsoft\\WindowsApps", program: "python.exe", scope: "user", kind: "store-alias" },
  ],
  firstIsDefault: true,
  pyLauncher: "C:\\Windows\\py.exe",
  overrides: [],
  ...patch,
});
const installed = [{ version: "3.13.14", path: root + "versions\\3.13.14", isDefault: true }, { version: "3.12.9", path: root + "versions\\3.12.9", isDefault: false }];

describe("what an AI is told about Python", () => {
  it("names the default interpreter, where pyenv lives and how python is found", () => {
    const text = pythonSummary(report(), installed, "3.1.1", "官方");
    expect(text).toContain(`解释器：${root}versions\\3.13.14\\python.exe`);
    expect(text).toContain(`shims 目录：${root}shims`);
    expect(text).toContain("系统 PATH 在前、用户 PATH 在后");
    expect(text).toContain("PowerShell 不查当前目录");
    expect(text).toContain("Get-Command python -All");
    expect(text).toContain("3.12.9：" + root + "versions\\3.12.9");
    expect(text).toContain("Microsoft Store 占位程序");
    expect(text).toContain(`"${root}versions\\3.13.14\\python.exe" -m pip install`);
    expect(text).toContain("不要自己搜索、下载或改用其他 Python");
    expect(text).toContain("python 命中的就是上面的默认 Python");
  });

  it("warns when another python is found first", () => {
    const text = pythonSummary(report({
      firstIsDefault: false,
      pathPythons: [{ dir: "C:\\Program Files\\Python312", program: "python.exe", scope: "system", kind: "other" }],
    }), installed, "3.1.1", "官方");
    expect(text).toContain("⚠ 结论：新打开的终端里，python 会先命中 C:\\Program Files\\Python312\\python.exe，不是默认 Python");
    expect(text).toContain("[系统 PATH]");
  });

  it("names what start python opens", () => {
    const text = pythonSummary(report({ appPathsPython: "C:\Store\python.exe" }), installed, "3.1.1", "官方");
    expect(text).toContain("App Paths 登记的：C:\Store\python.exe");
  });

  it("says there is no default instead of inventing one", () => {
    const text = pythonSummary(report({ defaultPython: null, defaultVersion: null, defaultDir: null, defaultScripts: null }), [], "3.1.1", "官方");
    expect(text).toContain("默认 Python：未设置");
    expect(text).toContain("请先让用户在 Stacker 的 Python 页面安装并设为默认版本");
    expect(text).not.toContain("-m pip install");
  });
});
