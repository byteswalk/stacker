export type AgentName = "codex" | "claude" | "codebuddy" | "workbuddy" | "workbuddy-ai" | "qoder" | "qoder-cn" | "mimo" | "kimi";
export type ClientTag = "desktop" | "terminal" | "ide" | "automation" | "sdk" | "unknown";
export type SessionStatus = "active" | "archived" | "orphaned" | "discarded";
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
  /** Set when this conversation was imported from an agent whose own copy is gone. */
  importedFrom?: AgentName | null;
  /** Agents holding an imported copy of this conversation; counted here, not twice. */
  importedBy?: AgentName[];
  summaryBy: string;
  summaryAt: number;
  /** Older transcripts of the same session in other worktree folders. */
  copies: string[];
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

export type AgentCount = { agent: AgentName; sessions: number; bytes: number };
export type SessionPage = { items: Session[]; total: number; ids: string[]; totalBytes: number; warnings: string[]; agents: AgentCount[] };
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
export const EMPTY_PAGE: SessionPage = { items: [], total: 0, ids: [], totalBytes: 0, warnings: [], agents: [] };
export const PAGE_SIZE = 40;

export const ERRORS: Record<string, string> = {
  E_STORAGE: "无法读写 Stacker 的会话数据目录，请检查剩余空间和目录权限。",
  E_SOURCE_MISSING: "数据目录不存在，请在「设置 → 高级：读取位置」中检查路径。",
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
  E_NOT_MOVABLE: "该目录已经不在默认位置（链接或环境变量），不能在这里迁移。",
  E_LINK_INSIDE: "目录里有无法重建的文件链接，已停止。",
  E_TARGET: "目标必须是绝对路径、为空或不存在，且不能与原目录互相包含。",
  E_TARGET_FS: "目标必须位于本机 NTFS 分区。",
  E_SPACE: "目标分区剩余空间不足。",
  E_LINK_CREATE: "无法创建目录联接，已恢复原状。",
  E_LINK_REMOVE: "无法删除目录联接。",
  E_NOT_LINK: "原位置不是目录联接。",
  E_REGISTRY: "无法写入注册表，请检查当前用户的注册表权限。",
  E_NO_EXTENSION: "没有找到插件文件夹。",
  E_NO_BODY: "Stacker 还没有这条对话的正文。",
  E_SUMMARY_BUSY: "已有网页对话摘要正在生成，请等待完成。",
  E_DISTILL_BUSY: "已有提炼任务正在执行，请等待完成或取消。",
};

export function errorMessage(error: unknown): string {
  const text = String(error);
  return ERRORS[text] ?? text;
}

export type FootprintKind = "sessions" | "reclaimable" | "review" | "keep";
export type FootprintItem = {
  id: string;
  /** Product id; the group it sits in carries the name and icon. */
  product: string;
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
/** One product in the agents catalogue, as the footprint page names it. */
export type ProductRef = { id: string; name: string; icon: string; sessionsAgent: AgentName | null };
export type AgentFootprint = { product: ProductRef; total: number; reclaimable: number; items: FootprintItem[] };
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

export type Volume = { root: string; fileSystem: string; free: number; fixed: boolean };
export type MigrationStep = "copying" | "copied" | "renamed" | "linked" | "done" | "cleaned";
export type LocationStatus = {
  agent: AgentName; source: string; actual: string;
  kind: "normal" | "migrated" | "incomplete" | "external_link" | "env" | "missing";
  step: MigrationStep | null; target: string; backup: string; backupExists: boolean; suggestedTarget: string; drives: Volume[];
};
export type MigrationCheck = { problems: string[]; bytes: number; files: number; free: number; target: string };
export type MigrationJob = { agent: AgentName | null; action: "migrate" | "back"; state: string; copied: number; total: number; error: string };

export type WebBrowser = "chrome" | "edge";
export type WebHostState = "off" | "connected" | "stale";
export type WebBrowserStatus = { browser: WebBrowser; state: WebHostState; registered: string };
export type WebCounts = { accounts: number; conversations: number; bodies: number; folders: number; excerpts: number };
/** Web chat times are milliseconds (browser clocks), unlike local sessions. */
export type WebchatStatus = {
  extensionId: string;
  extensionDir: string;
  extensionFound: boolean;
  dataDir: string;
  exportDir: string;
  browsers: WebBrowserStatus[];
  lastHelloAt: number | null;
  lastSyncAt: number | null;
  counts: WebCounts;
};
export type WebQuery = { site: string; account: string; search: string; fullText: boolean; offset: number };
export const EMPTY_WEB_QUERY: WebQuery = { site: "", account: "", search: "", fullText: false, offset: 0 };
export const WEB_PAGE_SIZE = 100;
export type WebChat = {
  key: string;
  site: string;
  account: string;
  accountName: string;
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  archived: boolean;
  removedAt: number | null;
  folder: string | null;
  tags: string[];
  favorite: boolean;
  note: string;
  bodyFetchedAt: number | null;
  bodyMessages: number;
  bodyStale: boolean;
  summary: string | null;
  summaryBy: string;
  summaryAt: number;
  summaryStale: boolean;
};
export type WebAccountOption = { key: string; site: string; name: string };
export type WebPage = { items: WebChat[]; total: number; accounts: WebAccountOption[] };
export type WebMessage = { role: string; text: string; at: number | null; attachments: string[] };
export type WebChatDetail = { chat: WebChat; messages: WebMessage[]; chars: number; runner: RunnerChoice };

const WEB_SITE_LABEL: Record<string, string> = { chatgpt: "ChatGPT", claude: "Claude", gemini: "Gemini", grok: "Grok", deepseek: "DeepSeek" };
/** Site ids come from the extension; unknown ones are shown as they are. */
export const webSiteLabel = (site: string) => WEB_SITE_LABEL[site] ?? site;

export type DistillKind = "qa" | "requirement" | "prompt" | "skill";
export const DISTILL_KINDS: DistillKind[] = ["qa", "requirement", "prompt", "skill"];
export const DISTILL_KIND_LABEL: Record<DistillKind, string> = {
  qa: "经验问答", requirement: "领域要求", prompt: "提示词", skill: "skill 草稿",
};
export type DistillSourceKind = "web" | "session" | "excerpt";
export type DistillSourceRef = { kind: DistillSourceKind; key: string };
/** 一条结果的来源：`web:<site>:<id>` / `session:<agent>:<nativeId>` / `excerpt:<id>`。 */
export type DistillSource = { key: string; kind: string; title: string; link: string };
export type DistillResult = {
  id: string;
  kind: DistillKind;
  title: string;
  body: string;
  sources: DistillSource[];
  state: "draft" | "adopted";
  by: string;
  /** skill 草稿的文件夹名，其他类型为空。 */
  folder: string;
  createdAt: number;
  updatedAt: number;
};
export type DistillQuery = { kind: string; state: string; search: string; source: string };
export const EMPTY_DISTILL_QUERY: DistillQuery = { kind: "", state: "", search: "", source: "" };
export type DistillKindCounts = { qa: number; requirement: number; prompt: number; skill: number; total: number };
export type DistillPage = { items: DistillResult[]; total: number; counts: DistillKindCounts };
export type DistillCandidate = { kind: DistillSourceKind; key: string; title: string; subtitle: string; available: boolean };
export type DistillPreview = { items: { title: string; chars: number }[]; totalChars: number; runner: RunnerChoice; skipped: number };
export type DistillJob = {
  id: string; state: string; stage: string; done: number; total: number;
  saved: number; folders: string[]; dropped: number; error: string; by: string;
};

// Stacker 侧不枚举站点（与 webSiteLabel 一样，这张表只用于显示）。
const WEB_SITE_URL: Record<string, (id: string) => string> = {
  chatgpt: (id) => `https://chatgpt.com/c/${id}`,
  claude: (id) => `https://claude.ai/chat/${id}`,
  gemini: (id) => `https://gemini.google.com/app/${id.replace(/^c_/, "")}`,
  grok: (id) => `https://grok.com/c/${id}`,
  deepseek: (id) => `https://chat.deepseek.com/a/chat/s/${id}`,
};
/** 来源的网址；本机会话的记录路径不是网址，返回空串。 */
export function distillSourceUrl(source: DistillSource): string {
  if (/^https?:\/\//.test(source.link)) return source.link;
  const [kind, site, ...rest] = source.key.split(":");
  const id = rest.join(":");
  if (kind !== "web" || !site || !id) return "";
  return WEB_SITE_URL[site]?.(id) ?? "";
}
