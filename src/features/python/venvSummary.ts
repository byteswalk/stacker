/** What `python_venv_inspect` reports about a project's Python environment. */
export type VenvInfo = {
  project: string;
  /** `venv`, `embedded` (a Python shipped with the project), or `none`. */
  kind: "venv" | "embedded" | "none";
  dir: string | null;
  python: string | null;
  version: string | null;
  base: string | null;
  baseExists: boolean;
  pipIni: string | null;
};

export function kindName(kind: VenvInfo["kind"]) {
  if (kind === "venv") return "虚拟环境";
  if (kind === "embedded") return "项目自带运行时";
  return "没有环境";
}

/** Tells an AI agent to run this project with its own environment, by absolute path. */
export function venvSummary(info: VenvInfo): string {
  const embedded = info.kind === "embedded";
  const lines = [
    `## 这个项目的 Python 环境（${kindName(info.kind)}，由 Stacker 管理）`,
    "",
    `- 项目目录：${info.project}`,
    `- 环境目录：${info.dir ?? "无"}`,
    `- 解释器：${info.python ?? "缺失"}${info.version ? `（Python ${info.version}）` : ""}`,
  ];
  if (embedded) {
    lines.push("- 这是随项目分发的嵌入式 Python（目录里有 pythonXY._pth），自带依赖，不要把它当成系统 Python。");
  } else {
    lines.push(`- 创建自：${info.base ?? "未知"}${info.baseExists ? "" : "（已不存在，虚拟环境不能用）"}`);
  }
  if (info.pipIni) lines.push(`- 这个环境自己的 pip 配置：${info.pipIni}（只对它生效，优先级高于用户级 pip.ini）`);
  lines.push("", "## 给 AI 的要求（请严格遵守）", "");
  if (info.python && info.baseExists) {
    lines.push(
      `1. 在这个项目里运行 Python 一律用："${info.python}"，不要用 PATH 里的 python，也不要用默认 Python。`,
      `2. 安装依赖用 "${info.python}" -m pip install …，只装进这个环境。`,
      embedded
        ? "3. 不要修改这个运行时的 _pth 文件，也不要在它上面再建虚拟环境；它随项目一起分发。"
        : "3. 不要再创建新的虚拟环境，也不要改用 conda、py 启动器或其他 Python。",
      "4. 不需要激活虚拟环境：直接用上面的绝对路径即可（PowerShell 可能禁止运行激活脚本）。",
    );
  } else {
    lines.push("1. 这个环境已损坏或不完整。请让用户在 Stacker 的 Python 页面删除后重新创建，不要自己修复或改用其他 Python。");
  }
  return lines.join("\n");
}
