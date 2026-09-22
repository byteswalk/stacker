/** The facts `python_env_report` gathers: what runs when a terminal types `python`. */
export type PathPython = { dir: string; program: string; scope: "system" | "user"; kind: "default" | "pyenv-shim" | "store-alias" | "venv" | "other" };
export type PythonEnvReport = {
  pyenvRoot: string | null;
  pyenvBin: string | null;
  pyenvShims: string | null;
  defaultVersion: string | null;
  defaultDir: string | null;
  defaultPython: string | null;
  defaultScripts: string | null;
  pathPythons: PathPython[];
  firstIsDefault: boolean;
  pyLauncher: string | null;
  overrides: [string, string][];
  appPathsPython?: string | null;
};

const KIND_TEXT: Record<PathPython["kind"], string> = {
  default: "默认 Python（Stacker 设置）",
  "pyenv-shim": "pyenv shim，按 pyenv 设置的默认版本转发",
  "store-alias": "Microsoft Store 占位程序，运行会打开商店，不能用",
  venv: "某个虚拟环境，不是系统默认",
  other: "其他 Python，不是默认",
};

/**
 * What an AI agent needs to run the right Python: the default interpreter's absolute path,
 * where pyenv-win lives, how Windows resolves `python` (system PATH first, then user PATH),
 * which one wins today, and plain rules not to hunt for another one.
 */
export function pythonSummary(report: PythonEnvReport, installed: { version: string; path?: string | null; isDefault: boolean }[], pyenvVersion: string | null, downloadSource: string): string {
  const exe = report.defaultPython;
  const lines: string[] = ["## Python 环境（由 Stacker 通过 pyenv-win 管理）", ""];
  lines.push(`- pyenv-win：${pyenvVersion ?? "未安装"}${report.pyenvRoot ? `，根目录 ${report.pyenvRoot}` : ""}`);
  if (report.pyenvBin) lines.push(`  - 命令目录（pyenv.bat）：${report.pyenvBin}`);
  if (report.pyenvShims) lines.push(`  - shims 目录：${report.pyenvShims}`);
  if (exe) {
    lines.push(`- 默认 Python：${report.defaultVersion}`);
    lines.push(`  - 解释器：${exe}`);
    if (report.defaultScripts) lines.push(`  - pip 与脚本目录：${report.defaultScripts}`);
  } else {
    lines.push("- 默认 Python：未设置");
  }
  lines.push(`- 已安装版本：${installed.length ? "" : "无"}`);
  for (const v of installed) lines.push(`  - ${v.version}${v.isDefault ? "（默认）" : ""}${v.path ? `：${v.path}` : ""}`);
  lines.push(`- Python 下载源：${downloadSource}`);

  lines.push("", "## 新打开的终端里输入 python 时，按这个顺序查找（系统 PATH 在前、用户 PATH 在后，第一个命中的生效）", "");
  if (report.pathPythons.length === 0) {
    lines.push("- PATH 里没有任何 python。");
  } else {
    report.pathPythons.forEach((p, i) => {
      lines.push(`${i + 1}. [${p.scope === "system" ? "系统 PATH" : "用户 PATH"}] ${p.dir}\\${p.program}：${KIND_TEXT[p.kind]}`);
    });
    const first = report.pathPythons[0];
    lines.push("", report.firstIsDefault
      ? "结论：新打开的终端里，python 命中的就是上面的默认 Python。"
      : `⚠ 结论：新打开的终端里，python 会先命中 ${first.dir}\\${first.program}，不是默认 Python。请用下面的绝对路径。`);
  }
  lines.push(
    "",
    "查找规则的补充说明：",
    "- 终端的 PATH 继承自启动它的程序：从 IDE、AI 工具或旧窗口里开的终端，PATH 可能和上面不同（例如多了某个虚拟环境），以实际命令结果为准。",
    "- cmd 会先查当前目录，当前目录里有 python.exe 时它优先；PowerShell 不查当前目录。",
    "- 同一目录里按 PATHEXT 顺序匹配，python.exe 先于 python.bat。",
  );
  if (report.appPathsPython) lines.push(`- 「运行」对话框和 start python 不走 PATH，而是打开 App Paths 登记的：${report.appPathsPython}`);
  if (report.pyLauncher) lines.push(`- py 启动器（${report.pyLauncher}）按它自己的规则选版本，不代表默认 Python，不要用。`);
  if (report.overrides.length) {
    lines.push(`- ⚠ 设置了会改变 Python 行为的环境变量：${report.overrides.map(([k, v]) => `${k}=${v}`).join("；")}`);
  }

  lines.push("", "## 给 AI 的要求（请严格遵守）", "");
  if (exe) {
    lines.push(
      `1. 运行 Python 一律用默认解释器的绝对路径："${exe}"。`,
      `2. 安装依赖用 "${exe}" -m pip install …，不要直接调用 pip 或 pip3。`,
      `3. 项目需要隔离依赖时，用默认解释器创建虚拟环境："${exe}" -m venv .venv，之后使用 .venv\\Scripts\\python.exe。`,
      "4. 不要自己搜索、下载或改用其他 Python：不用 py 启动器，不用 Microsoft Store 版 python，不用 IDE 或其他工具自带的 Python。",
      "5. 需要别的 Python 版本时，请告诉用户在 Stacker 的 Python 页面安装并设为默认，不要修改 PATH、注册表或 pyenv 配置。",
      `6. 不确定时，在你自己的终端里核对：cmd 用 where python，PowerShell 用 Get-Command python -All，再运行 "${exe}" --version。`,
    );
  } else {
    lines.push(
      "1. 当前没有设置默认 Python。请先让用户在 Stacker 的 Python 页面安装并设为默认版本，再继续。",
      "2. 不要自己搜索、下载或改用其他 Python，也不要修改 PATH、注册表或 pyenv 配置。",
    );
  }
  return lines.join("\n");
}
