export type AgentName = "codex" | "claude";
export type ClientTag = "desktop" | "terminal" | "ide" | "automation" | "sdk" | "unknown";
export type SessionStatus = "active" | "archived" | "orphaned";
export type TitleSource = "client" | "custom" | "summary" | "first_message";
export type DeleteMode = "slim_export" | "direct" | "full_backup";

export type ProjectRef = { key: string; name: string; path: string; exists: boolean };
export type ChildSummary = { id: string; kind: string; title: string; bytes: number; path: string };

export type Session = {
  id: string;
  agent: AgentName;
  nativeId: string;
  title: string;
  titleSource: TitleSource;
  project: ProjectRef;
  client: ClientTag;
  createdAt: number;
  updatedAt: number;
  archived: boolean;
  pinned: boolean;
  status: SessionStatus;
  children: ChildSummary[];
  bytes: number;
  path: string;
  inDesktopIndex: boolean;
  parentMissing: boolean;
  favorite: boolean;
  summary: string | null;
  summaryStale: boolean;
  summaryBy: string;
  summaryAt: number;
};

export type SessionQuery = {
  agent: string;
  project: string;
  status: string;
  client: string;
  search: string;
  fullText: boolean;
  includeAutomation: boolean;
  favoritesOnly: boolean;
  updatedAfter: number;
  sort: "" | "bytes";
  offset: number;
};

export type SessionPage = { items: Session[]; total: number; ids: string[]; totalBytes: number; warnings: string[] };
export type ProjectRow = { project: ProjectRef; agents: AgentName[]; sessions: number; orphans: number; bytes: number; updatedAt: number };
export type Roots = { codex: string; claude: string; claudeDesktopIndex: string };
export type RootsView = { effective: Roots; overrides: Roots; exportDir: string };
export type Message = { line: number; role: string; text: string };
export type SessionDetail = { session: Session; messages: Message[]; total: number; complete: boolean };
export type Blocked = { id: string; title: string; reason: string };
export type DeletePreview = {
  token: string;
  mode: DeleteMode;
  sessions: Session[];
  children: number;
  files: number;
  bytes: number;
  blocked: Blocked[];
  exportDir: string;
  created: number;
};
export type DeleteJobItem = { id: string; title: string; status: string; detail: string };
export type DeleteJob = { id: string; state: string; done: number; total: number; items: DeleteJobItem[]; exportDir: string; error: string };

export const EMPTY_QUERY: SessionQuery = {
  agent: "", project: "", status: "", client: "", search: "", fullText: false,
  includeAutomation: false, favoritesOnly: false, updatedAfter: 0, sort: "", offset: 0,
};
export const EMPTY_PAGE: SessionPage = { items: [], total: 0, ids: [], totalBytes: 0, warnings: [] };
export const PAGE_SIZE = 40;

export const ERRORS: Record<string, string> = {
  E_STORAGE: "无法读写 Stacker 的会话数据目录，请检查剩余空间和目录权限。",
  E_SOURCE_MISSING: "数据目录不存在，请在「数据来源」中检查路径。",
  E_PATH: "路径不存在或不在允许范围内。",
  E_LINK: "涉及符号链接或目录联接，已阻止操作。",
  E_ACCESS: "文件访问被拒绝，请检查权限。",
  E_NOT_FOUND: "会话已不存在，请刷新列表。",
  E_BUSY: "已有删除任务正在执行，请等待或取消。",
  E_REQUEST: "请求无效（单次最多 500 条），请调整选择后重试。",
  E_PREVIEW: "删除预览已过期或已执行，请重新预览。",
  E_CHANGED: "会话在预览后发生了变化，请刷新后重新预览。",
  E_IN_DESKTOP: "该会话仍在 Claude 桌面端侧栏中，请先在桌面端删除。",
  E_IN_USE: "该会话最近 2 分钟内仍有写入，可能正在使用。",
  E_CLOSE_CODEX: "请先完全退出 Codex 桌面端和 CLI。",
  E_PROCESS_CHECK: "无法确认智能体是否已退出，已阻止操作。",
  E_CODEX_MISSING: "未找到可用的 Codex CLI，请在「安装更新」页安装。",
  E_CODEX_VERSION: "本机 Codex 版本过旧，不支持安全删除，请先更新。",
  E_RPC: "Codex 接口未完成操作，请查看任务结果。",
  E_VERIFY: "删除后核对未通过，请刷新检查。",
  E_TIMEOUT: "接口响应超时，请刷新核对结果。",
  E_CANCELLED: "操作已取消，已完成的项目保留。",
  E_APP_RUNNING: "对应程序正在运行，请先退出后再清理。",
  E_RUNNER_MISSING: "未找到该智能体的命令行，请在「安装更新」页安装，或在摘要设置中改用另一个。",
  E_RUNNER_AUTH: "该智能体尚未登录，请先在终端运行它并完成登录。",
  E_RUNNER_TIMEOUT: "智能体 5 分钟内没有完成，已停止。",
  E_RUNNER_FAILED: "智能体运行失败，请稍后重试或换一个模型。",
  E_RUNNER_EMPTY: "智能体没有返回内容。",
};

export function errorMessage(error: unknown): string {
  const text = String(error);
  return ERRORS[text] ?? text;
}

export type FootprintKind = "sessions" | "reclaimable" | "review" | "keep";
export type FootprintItem = {
  id: string;
  agent: AgentName;
  owner: "shared" | "desktop_app";
  kind: FootprintKind;
  label: string;
  explain: string;
  paths: string[];
  bytes: number;
  files: number;
  blocked: string | null;
  note: string | null;
};
export type AgentFootprint = { agent: AgentName; total: number; reclaimable: number; items: FootprintItem[] };
export type FootprintReport = { agents: AgentFootprint[]; total: number; reclaimable: number; scannedAt: number; warnings: string[] };
export type CleanupPreview = { token: string; items: FootprintItem[]; blocked: FootprintItem[]; bytes: number; created: number };
export type CleanupItemResult = { id: string; label: string; status: string; detail: string; freed: number };
export type CleanupJob = { state: string; done: number; total: number; freed: number; items: CleanupItemResult[]; error: string };

export type SummarySettings = { runner: "same" | "codex" | "claude"; codexModel: string; codexEffort: string; claudeModel: string; claudeEffort: string };
export type RunnerChoice = { agent: AgentName; model: string | null; effort: string | null };
export type ModelOption = { id: string; label: string; efforts: string[]; defaultEffort: string | null };
export type AgentOptions = { agent: AgentName; installed: boolean; models: ModelOption[]; efforts: string[] };
export type SummaryPreviewItem = { id: string; title: string; agent: AgentName; chars: number; needed: boolean; runner: RunnerChoice };
export type SummaryPreview = { items: SummaryPreviewItem[]; totalChars: number; projectName: string; handoffRunner: RunnerChoice | null };
export type SummaryJobItem = { id: string; title: string; status: string; detail: string; elapsedMs: number; by: string };
export type SummaryJob = { id: string; kind: "summary" | "handoff"; state: string; done: number; total: number; items: SummaryJobItem[]; error: string; resultPath: string; resultText: string };
