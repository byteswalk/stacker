/** What `python_venv_inspect` reports about a project's virtual environment. */
export type VenvInfo = {
  project: string;
  dir: string | null;
  python: string | null;
  version: string | null;
  base: string | null;
  baseExists: boolean;
};

/** Tells an AI agent to run this project with its own venv, by absolute path. */
export function venvSummary(info: VenvInfo): string {
  const lines = [
    "## 这个项目的 Python 虚拟环境（由 Stacker 管理）",
    "",
    `- 项目目录：${info.project}`,
    `- 虚拟环境：${info.dir ?? "无"}`,
    `- 解释器：${info.python ?? "缺失"}${info.version ? `（Python ${info.version}）` : ""}`,
    `- 创建自：${info.base ?? "未知"}${info.baseExists ? "" : "（已不存在，虚拟环境不能用）"}`,
    "",
    "## 给 AI 的要求（请严格遵守）",
    "",
  ];
  if (info.python && info.baseExists) {
    lines.push(
      `1. 在这个项目里运行 Python 一律用："${info.python}"，不要用 PATH 里的 python，也不要用默认 Python。`,
      `2. 安装依赖用 "${info.python}" -m pip install …，只装进这个虚拟环境。`,
      "3. 不要再创建新的虚拟环境，也不要改用 conda、py 启动器或其他 Python。",
      "4. 不需要激活虚拟环境：直接用上面的绝对路径即可（PowerShell 可能禁止运行激活脚本）。",
    );
  } else {
    lines.push("1. 这个虚拟环境已损坏或不完整。请让用户在 Stacker 的 Python 页面删除后重新创建，不要自己修复或改用其他 Python。");
  }
  return lines.join("\n");
}
