import { useCallback, useEffect, useState } from "react";
import { invoke } from "../invoke";
import { open } from "@tauri-apps/plugin-dialog";
import { TerminalBar } from "../TerminalBar";
import { Select } from "../Select";
import { ConfirmModal, ErrorState, Loading, Modal, useBusy, useToast } from "../ui";
import { remoteRepositoryCreationHint, supportsRemoteRepositoryCreation } from "../gitCapabilities";
import { translateText } from "../i18n";

type Shells = { powershell: boolean; gitbash: boolean; cmd: boolean };
type GitStatus = {
  installed: boolean;
  version?: string | null;
  path?: string | null;
  bash_path?: string | null;
  credential_helper?: string | null;
  http_proxy?: string | null;
  https_proxy?: string | null;
  user_name?: string | null;
  user_email?: string | null;
  default_branch?: string | null;
  autocrlf?: string | null;
  gcm: boolean;
};
type GitHubAccountsState = {
  gcm_available: boolean;
  gcm_version?: string | null;
  accounts: string[];
};
type GitAccountProfile = {
  platform: string;
  username: string;
  display_name?: string | null;
  email?: string | null;
  expires_at?: string | null;
  authenticated: boolean;
  token_verified?: boolean;
  service_name?: string | null;
  base_url?: string | null;
  provider?: string | null;
};
type GitUpdateInfo = { current: string; latest: string; has_update: boolean; source_name: string; release_url: string; installer_url: string };
type GitInitResult = { directory: string; remote_url?: string | null; initial_commit: boolean };
type GitMigrationResult = { mode: "native_transfer" | "git_mirror"; message: string };
type Mirror = { id: string; name: string; url: string; host: string };
type ToolState = { id: string; mirrors: Mirror[] };
type HostPing = { host: string; ms: number | null };

const EMPTY_GITHUB: GitHubAccountsState = { gcm_available: false, gcm_version: null, accounts: [] };
const GIT_SOURCE_KEY = "stacker.git.downloadSource";
const FALLBACK_GIT_SOURCES: Mirror[] = [
  { id: "official", name: "官方", url: "https://github.com/git-for-windows/git/releases/latest", host: "github.com" },
  { id: "npmmirror", name: "npmmirror", url: "https://registry.npmmirror.com/-/binary/git-for-windows/", host: "registry.npmmirror.com" },
  { id: "tuna", name: "清华", url: "https://mirrors.tuna.tsinghua.edu.cn/github-release/git-for-windows/git/", host: "mirrors.tuna.tsinghua.edu.cn" },
  { id: "huawei", name: "华为云", url: "https://repo.huaweicloud.com/git-for-windows/", host: "repo.huaweicloud.com" },
];

function initialGitSource() {
  return localStorage.getItem(GIT_SOURCE_KEY) || "official";
}

function accountKey(account: GitAccountProfile) {
  return `${account.platform}:${account.username}`;
}

function platformName(platform: string) {
  if (platform === "github") return "GitHub";
  if (platform === "gitee") return "Gitee";
  return "其他 Git 服务";
}

function accountPlatformName(account: GitAccountProfile) {
  return account.service_name?.trim() || platformName(account.platform);
}

function accountOptions(accounts: GitAccountProfile[]) {
  return accounts.map((account) => ({
    value: accountKey(account),
    label: `${accountPlatformName(account)} · ${account.username}`,
  }));
}

function tokenExpiry(account: GitAccountProfile) {
  if (!account.expires_at) return null;
  const normalized = account.expires_at.includes("T")
    ? account.expires_at
    : account.expires_at.replace(/ UTC$/i, "Z").replace(" ", "T");
  const expires = new Date(normalized);
  if (Number.isNaN(expires.getTime())) return null;
  const date = new Intl.DateTimeFormat("zh-CN", { year: "numeric", month: "2-digit", day: "2-digit" }).format(expires);
  const days = Math.ceil((expires.getTime() - Date.now()) / 86_400_000);
  if (days < 0) return { className: "r", label: "令牌（token）已过期", detail: `令牌（token）到期时间：${date}` };
  if (days <= 30) return { className: "w", label: `令牌（token）${days} 天后到期`, detail: `令牌（token）到期时间：${date}` };
  return { className: "g", label: `令牌（token）有效至 ${date}`, detail: `令牌（token）到期时间：${date}` };
}

export default function Git() {
  const toast = useToast();
  const runBusy = useBusy();
  const [status, setStatus] = useState<GitStatus | null>(null);
  const [github, setGithub] = useState<GitHubAccountsState>(EMPTY_GITHUB);
  const [accounts, setAccounts] = useState<GitAccountProfile[]>([]);
  const [shells, setShells] = useState<Shells>({ powershell: true, gitbash: false, cmd: true });
  const [accountToken, setAccountToken] = useState("");
  const [addProvider, setAddProvider] = useState<"github" | "gitee" | "custom" | null>(null);
  const [customServiceUrl, setCustomServiceUrl] = useState("");
  const [customServiceName, setCustomServiceName] = useState("");
  const [customUsername, setCustomUsername] = useState("");
  const [updateInfo, setUpdateInfo] = useState<GitUpdateInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const [downloadSources, setDownloadSources] = useState<Mirror[]>(FALLBACK_GIT_SOURCES);
  const [downloadSource, setDownloadSource] = useState(initialGitSource);
  const [pendingDownloadSource, setPendingDownloadSource] = useState(initialGitSource);
  const [sourcePings, setSourcePings] = useState<Record<string, number | null>>({});
  const [loadError, setLoadError] = useState(false);
  const [removeAccount, setRemoveAccount] = useState<GitAccountProfile | null>(null);
  const [editAccount, setEditAccount] = useState<GitAccountProfile | null>(null);
  const [editName, setEditName] = useState("");
  const [editEmail, setEditEmail] = useState("");
  const [initAccount, setInitAccount] = useState<GitAccountProfile | null>(null);
  const [initDirectory, setInitDirectory] = useState("");
  const [repositoryName, setRepositoryName] = useState("");
  const [description, setDescription] = useState("");
  const [privateRepository, setPrivateRepository] = useState(false);
  const [createRemote, setCreateRemote] = useState(true);
  const [remoteUrl, setRemoteUrl] = useState("");
  const [createReadme, setCreateReadme] = useState(true);
  const [migrationOpen, setMigrationOpen] = useState(false);
  const [sourceAccount, setSourceAccount] = useState("");
  const [targetAccount, setTargetAccount] = useState("");
  const [sourceOwner, setSourceOwner] = useState("");
  const [sourceRepository, setSourceRepository] = useState("");
  const [targetRepository, setTargetRepository] = useState("");
  const [targetPrivate, setTargetPrivate] = useState(false);
  const [includeLfs, setIncludeLfs] = useState(true);

  const loadAccounts = useCallback(async () => {
    const [githubState, profiles] = await Promise.all([
      invoke<GitHubAccountsState>("git_github_accounts").catch(() => EMPTY_GITHUB),
      invoke<GitAccountProfile[]>("git_account_profiles").catch(() => []),
    ]);
    setGithub(githubState);
    setAccounts(profiles);
  }, []);

  const load = useCallback(async () => {
    const [nextStatus, nextShells] = await Promise.all([
      invoke<GitStatus>("git_status"),
      invoke<Shells>("shells_available").catch(() => ({ powershell: true, gitbash: false, cmd: true })),
    ]);
    setStatus(nextStatus);
    setShells(nextShells);
    if (nextStatus.installed) await loadAccounts();
    else {
      setGithub(EMPTY_GITHUB);
      setAccounts([]);
    }
    setLoadError(false);
  }, [loadAccounts]);

  useEffect(() => {
    load().catch(() => setLoadError(true));
  }, [load]);

  useEffect(() => {
    invoke<ToolState[]>("list_sources").then((tools) => {
      const mirrors = tools.find((tool) => tool.id === "git-runtime")?.mirrors;
      const next = mirrors?.length ? mirrors : FALLBACK_GIT_SOURCES;
      setDownloadSources(next);
      setDownloadSource((current) => {
        if (next.some((mirror) => mirror.id === current)) return current;
        const fallback = next.find((mirror) => mirror.id === "official")?.id || next[0]?.id || "official";
        localStorage.setItem(GIT_SOURCE_KEY, fallback);
        setPendingDownloadSource(fallback);
        if (current) toast("原 Git 下载源已不在源清单中，已恢复为官方源", "info");
        return fallback;
      });
      setPendingDownloadSource((current) => next.some((mirror) => mirror.id === current)
        ? current
        : (next.find((mirror) => mirror.id === "official")?.id || next[0]?.id || "official"));
    }).catch(() => undefined);
  }, [toast]);

  const proxyConfigured = !!(status?.http_proxy || status?.https_proxy);
  const allAccountOptions = accountOptions(accounts);
  const sourceName = (id: string) => downloadSources.find((source) => source.id === id)?.name || id;
  const fastestSource = Object.entries(sourcePings)
    .filter((entry): entry is [string, number] => typeof entry[1] === "number")
    .sort((a, b) => a[1] - b[1])[0]?.[0];
  const sourceOptions = downloadSources.map((source) => {
    const tested = source.id in sourcePings;
    const ms = sourcePings[source.id];
    const suffix = !tested ? "" : ms === null ? " · 超时" : ` · ${ms}ms${source.id === fastestSource ? " · 最快" : ""}`;
    return { value: source.id, label: `${source.name}${suffix}` };
  });

  const gitSummary = [
    "## Git 开发生态摘要",
    `- Git：${status?.installed ? "可用" : "未安装"}${status?.version ? `，版本：${status.version}` : ""}${status?.path ? `，路径：${status.path}` : ""}`,
    `- Git Bash：${status?.bash_path || "未检测到"}`,
    `- Git Credential Manager：${github.gcm_available ? "可用" : "未检测到"}${github.gcm_version ? `，版本：${github.gcm_version}` : ""}`,
    `- 普通终端默认提交身份：${status?.user_name || "未配置"} <${status?.user_email || "未配置"}>`,
    `- 默认分支：${status?.default_branch || "未配置"}`,
    `- autocrlf：${status?.autocrlf || "未配置"}`,
    `- 已管理账号：${accounts.length ? accounts.map((account) => {
      const identity = account.display_name && account.email ? `，提交身份 ${account.display_name} <${account.email}>` : "，提交身份未配置";
      return `${accountPlatformName(account)}:${account.username}${identity}`;
    }).join("；") : "无"}`,
    "",
    "给 AI 的使用建议：未指定账号时优先按普通终端默认 Git 配置操作；用户指定某个账号时，应使用该账号专属终端或该账号摘要中的 HTTPS 地址规则操作。不要读取、输出或索要访问令牌（token）。",
  ].join("\n");

  function applyDownloadSource() {
    if (!downloadSources.some((source) => source.id === pendingDownloadSource)) return;
    setDownloadSource(pendingDownloadSource);
    localStorage.setItem(GIT_SOURCE_KEY, pendingDownloadSource);
    setUpdateInfo(null);
    toast(`已应用 Git 下载源：${sourceName(pendingDownloadSource)}`, "ok");
  }

  async function speedtestSources() {
    setBusy(true);
    try {
      const hosts = [...new Set(downloadSources.map((source) => source.host).filter(Boolean))];
      const rows = await runBusy(
        { title: "Git 下载源测速", message: "正在测试各下载源的连接状态；单个源 1500ms 无响应算超时。" },
        () => invoke<HostPing[]>("speedtest_hosts", { hosts }),
      );
      const byHost = new Map(rows.map((row) => [row.host, row.ms]));
      const nextPings: Record<string, number | null> = {};
      downloadSources.forEach((source) => { nextPings[source.id] = byHost.get(source.host) ?? null; });
      setSourcePings(nextPings);
      const fastest = downloadSources
        .map((source) => ({ source, ms: nextPings[source.id] }))
        .filter((item): item is { source: Mirror; ms: number } => typeof item.ms === "number")
        .sort((a, b) => a.ms - b.ms)[0];
      if (fastest) {
        setPendingDownloadSource(fastest.source.id);
        toast(
          fastest.source.id === downloadSource
            ? `测速完成，${fastest.source.name} 已是当前下载源`
            : `测速完成，已预选 ${fastest.source.name}，点击「应用」后生效`,
          "ok",
        );
      } else {
        toast("Git 下载源测速均超时，保留当前下载源", "info");
      }
    } catch (error) {
      toast("Git 下载源测速失败。请检查网络连接后重试。原因：" + error, "err");
    } finally {
      setBusy(false);
    }
  }

  async function openUrl(url: string) {
    try {
      await invoke("app_open_url", { url });
    } catch (error) {
      toast("无法打开链接：" + error, "err");
    }
  }

  async function refresh() {
    try {
      await load();
      toast("Git 环境和账号状态已刷新", "ok");
    } catch (error) {
      toast("刷新 Git 状态失败：" + error, "err");
    }
  }

  async function checkUpdate() {
    if (!status?.installed) return toast("请先安装 Git for Windows", "info");
    setBusy(true);
    try {
      const info = await runBusy(
        { title: "检查 Git for Windows 更新", message: "正在查询最新正式版本。" },
        () => invoke<GitUpdateInfo>("git_check_update", { sourceId: downloadSource }),
      );
      setUpdateInfo(info.has_update ? info : null);
      toast(info.has_update ? `发现 Git for Windows ${info.latest}` : "当前已是最新版本", "ok");
    } catch (error) {
      toast("检查 Git 更新失败：" + error, "err");
    } finally {
      setBusy(false);
    }
  }

  async function installGit() {
    const updating = !!status?.installed;
    const systemUpdate = updating && /\\program files(?: \(x86\))?\\/i.test(status?.path || "");
    setBusy(true);
    let cancelled = false;
    try {
      const result = await runBusy(
        {
          title: updating ? "更新 Git for Windows" : "安装 Git for Windows",
          message: `Stacker 将从「${sourceName(downloadSource)}」获取 Git for Windows 正式版并校验官方数字签名。${systemUpdate ? "当前为系统级安装，更新时 Windows 将请求一次管理员授权。" : "将为当前用户静默安装 Git、Git Bash 与 GCM，无需管理员授权。"}`,
          progressEvent: "install-progress",
          cancel: {
            label: updating ? "取消更新" : "取消安装",
            onCancel: () => {
              cancelled = true;
              invoke("op_cancel").catch(() => undefined);
            },
          },
        },
        async () => {
          const message = await invoke<string>("git_install", { sourceId: downloadSource });
          await load();
          return message;
        },
      );
      setUpdateInfo(null);
      toast(result || (updating ? "Git for Windows 已更新" : "Git for Windows 已安装"), "ok");
    } catch (error) {
      const detail = String(error);
      if (cancelled || detail.includes("已取消")) {
        toast(updating ? "已取消 Git 更新" : "已取消 Git 安装", "info");
      } else {
        toast((updating ? "Git 更新失败：" : "Git 安装失败：") + detail, "err");
      }
    } finally {
      setBusy(false);
    }
  }

  async function saveTokenAccount() {
    if (!addProvider || !accountToken.trim()) return toast("请填写访问令牌（token）", "info");
    if (addProvider === "custom" && (!customServiceUrl.trim() || !customUsername.trim())) {
      return toast("请填写 Git 服务地址和账号名称", "info");
    }
    const provider = addProvider === "custom" ? (customServiceName.trim() || "其他 Git 服务") : platformName(addProvider);
    setBusy(true);
    try {
      const profiles = await runBusy(
        { title: `添加 ${provider} 账号`, message: "正在验证访问令牌（token）并读取账号信息。验证通过后，令牌（token）将保存到 Windows 凭据管理器。" },
        () => addProvider === "custom"
          ? invoke<GitAccountProfile[]>("git_account_save_custom_token", {
            serviceUrl: customServiceUrl.trim(),
            serviceName: customServiceName.trim(),
            username: customUsername.trim(),
            credential: accountToken.trim(),
          })
          : invoke<GitAccountProfile[]>("git_account_save_token", { platform: addProvider, credential: accountToken.trim() }),
      );
      setAccounts(profiles);
      await loadAccounts();
      setAccountToken("");
      setCustomServiceUrl("");
      setCustomServiceName("");
      setCustomUsername("");
      setAddProvider(null);
      toast(`${provider} 账号已添加`, "ok");
    } catch (error) {
      toast(`添加 ${provider} 账号失败：${error}`, "err");
    } finally {
      setBusy(false);
    }
  }

  async function confirmRemoveAccount() {
    if (!removeAccount) return;
    setBusy(true);
    try {
      await invoke("git_account_remove_token", { platform: removeAccount.platform, username: removeAccount.username });
      setRemoveAccount(null);
      await loadAccounts();
      toast(`${accountPlatformName(removeAccount)} 账号已从本机移除`, "ok");
    } catch (error) {
      toast("移除账号失败：" + error, "err");
    } finally {
      setBusy(false);
    }
  }

  function beginEditIdentity(account: GitAccountProfile) {
    setEditAccount(account);
    setEditName(account.display_name || "");
    setEditEmail(account.email || "");
  }

  async function saveIdentity(close = true) {
    if (!editAccount) return false;
    if (!editName.trim() || !editEmail.trim()) {
      toast("请填写提交姓名和邮箱", "info");
      return false;
    }
    setBusy(true);
    try {
      const profiles = await invoke<GitAccountProfile[]>("git_account_save_identity", {
        platform: editAccount.platform,
        username: editAccount.username,
        displayName: editName.trim(),
        email: editEmail.trim(),
      });
      setAccounts(profiles);
      if (close) setEditAccount(null);
      toast("账号提交身份已保存", "ok");
      return true;
    } catch (error) {
      toast("保存提交身份失败：" + error, "err");
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function pickDirectory(title: string) {
    const selected = await open({ directory: true, multiple: false, title });
    return typeof selected === "string" ? selected : null;
  }

  async function openAccountTerminal(account: GitAccountProfile, kind: keyof Shells) {
    try {
      await invoke("git_account_open_shell", {
        platform: account.platform,
        username: account.username,
        kind,
        cwd: null,
      });
      toast(`已打开 ${accountPlatformName(account)} · ${account.username} 专属终端`, "ok");
    } catch (error) {
      toast("打开账号终端失败：" + error, "err");
    }
  }

  async function setAccountGlobal(account: GitAccountProfile) {
    try {
      await invoke("git_account_set_global", { platform: account.platform, username: account.username });
      await load();
      toast(`已将 ${accountPlatformName(account)} · ${account.username} 设为普通终端默认 Git 账号`, "ok");
    } catch (error) {
      toast("设置全局默认 Git 账号失败：" + error, "err");
    }
  }

  async function copyAccountContext(account: GitAccountProfile) {
    try {
      const content = await invoke<string>("git_account_ai_context", { platform: account.platform, username: account.username });
      await navigator.clipboard.writeText(translateText(content));
      toast(`已复制 ${account.username} 的摘要给 AI`, "ok");
    } catch (error) {
      toast("复制账号摘要给 AI 失败：" + error, "err");
    }
  }

  function beginInit(account: GitAccountProfile) {
    setInitAccount(account);
    setInitDirectory("");
    setRepositoryName("");
    setDescription("");
    setPrivateRepository(false);
    setCreateRemote(supportsRemoteRepositoryCreation(account));
    setRemoteUrl("");
    setCreateReadme(true);
    setEditName(account.display_name || account.username);
    setEditEmail(account.email || "");
  }

  async function chooseInitDirectory() {
    const directory = await pickDirectory("选择要初始化的工程目录");
    if (!directory) return;
    setInitDirectory(directory);
    if (!repositoryName) {
      const parts = directory.replace(/[\\/]+$/, "").split(/[\\/]/);
      setRepositoryName(parts[parts.length - 1] || "");
    }
  }

  async function initializeRepository() {
    if (!initAccount || !initDirectory || !repositoryName.trim()) return toast("请选择工程目录并填写仓库名称", "info");
    if (!editName.trim() || !editEmail.trim()) return toast("请填写该账号的提交姓名和邮箱", "info");
    const canCreateRemote = supportsRemoteRepositoryCreation(initAccount);
    const shouldCreateRemote = createRemote && canCreateRemote;
    setBusy(true);
    try {
      await invoke("git_account_save_identity", {
        platform: initAccount.platform,
        username: initAccount.username,
        displayName: editName.trim(),
        email: editEmail.trim(),
      });
      const result = await runBusy(
        {
          title: `使用 ${initAccount.username} 初始化工程`,
          message: shouldCreateRemote ? "将创建远程仓库、初始化本地目录并写入仓库级提交身份。" : "将初始化本地目录并写入仓库级提交身份。",
          progressEvent: "git-operation-progress",
        },
        () => invoke<GitInitResult>("git_init_repository", {
          request: {
            platform: initAccount.platform,
            username: initAccount.username,
            directory: initDirectory,
            repositoryName: repositoryName.trim(),
            description: description.trim(),
            privateRepository,
            createRemote: shouldCreateRemote,
            remoteUrl: remoteUrl.trim() || null,
            createReadme,
            displayName: editName.trim(),
            email: editEmail.trim(),
          },
        }),
      );
      await loadAccounts();
      setInitAccount(null);
      toast(result.remote_url ? "工程已初始化并连接远程仓库" : "本地工程已初始化", "ok");
    } catch (error) {
      toast("初始化工程失败：" + error, "err");
    } finally {
      setBusy(false);
    }
  }

  function selectedAccount(value: string) {
    return accounts.find((account) => accountKey(account) === value);
  }

  function beginMigration() {
    const source = accounts[0];
    const target = accounts[1] || source;
    setSourceAccount(source ? accountKey(source) : "");
    setTargetAccount(target ? accountKey(target) : "");
    setSourceOwner(source?.username || "");
    setSourceRepository("");
    setTargetRepository("");
    setTargetPrivate(false);
    setIncludeLfs(true);
    setMigrationOpen(true);
  }

  async function migrateRepository() {
    const source = selectedAccount(sourceAccount);
    const target = selectedAccount(targetAccount);
    if (!source || !target) return toast("请选择源账号和目标账号", "info");
    if (!sourceOwner.trim() || !sourceRepository.trim() || !targetRepository.trim()) return toast("请填写源仓库和目标仓库名称", "info");
    if (accountKey(source) === accountKey(target) && sourceOwner.trim().toLowerCase() === target.username.toLowerCase() && sourceRepository.trim().toLowerCase() === targetRepository.trim().toLowerCase()) {
      return toast("同一账号下，源仓库和目标仓库名称不能相同", "info");
    }
    setBusy(true);
    try {
      const result = await runBusy(
        {
          title: "迁移仓库",
          message: "Stacker 正在判断最佳迁移方式，并使用所选账号完成平台操作。",
          progressEvent: "git-operation-progress",
        },
        () => invoke<GitMigrationResult>("git_auto_migrate_repository", {
          request: {
            sourcePlatform: source.platform,
            sourceAccount: source.username,
            sourceOwner: sourceOwner.trim(),
            sourceRepository: sourceRepository.trim(),
            targetPlatform: target.platform,
            targetAccount: target.username,
            targetRepository: targetRepository.trim(),
            targetPrivate,
            includeLfs,
          },
        }),
      );
      toast(`${result.mode === "native_transfer" ? "GitHub 原生转移" : "Git 镜像迁移"}：${result.message}`, "ok");
      setMigrationOpen(false);
    } catch (error) {
      toast("仓库迁移失败：" + error, "err");
    } finally {
      setBusy(false);
    }
  }

  async function applyProxy() {
    setBusy(true);
    try {
      await runBusy({ title: "配置 Git 代理", message: "正在将设置页中的代理地址写入 Git 全局配置。" }, async () => {
        await invoke("git_apply_proxy");
        await load();
      });
      toast("Git 代理已配置", "ok");
    } catch (error) {
      toast("配置 Git 代理失败：" + error, "err");
    } finally {
      setBusy(false);
    }
  }

  async function clearProxy() {
    setBusy(true);
    try {
      await runBusy({ title: "清除 Git 代理", message: "正在移除 Git 全局代理配置。" }, async () => {
        await invoke("git_clear_proxy");
        await load();
      });
      toast("Git 代理已清除", "ok");
    } catch (error) {
      toast("清除 Git 代理失败：" + error, "err");
    } finally {
      setBusy(false);
    }
  }

  if (loadError) return <ErrorState title="暂时无法读取 Git 环境" description="请确认 Git 相关进程未被安全软件拦截，然后重试。" onRetry={load} />;
  const statusLoading = !status;
  const gitStatus: GitStatus = status ?? {
    installed: false,
    version: null,
    path: null,
    bash_path: null,
    credential_helper: null,
    user_name: null,
    user_email: null,
    default_branch: null,
    autocrlf: null,
    http_proxy: null,
    https_proxy: null,
    gcm: false,
  };

  return (
    <>
      {statusLoading ? (
        <Loading text="正在检测 Git for Windows、Git Bash、GCM 与账号配置…" />
      ) : (
        <TerminalBar
          avail={shells}
          ecosystem="git"
          summary={gitSummary}
          tip="这里打开普通 Git 终端并打印 Git 环境摘要；账号卡片中的终端会创建与所选账号绑定的独立 Git 操作环境。"
        />
      )}

      <div className="srcrow" style={{ marginBottom: 10 }}>
        <span className="av st"><i className="ti ti-download" /></span>
        <div className="mt">
          <div className="t">Git 下载源</div>
          <div className="s dim" title="用于检查更新以及下载 Git for Windows 安装程序。测速只会预选下载源，点击「应用」后生效。">用于安装和更新 Git；测速后点击「应用」生效。</div>
        </div>
        <Select value={pendingDownloadSource} width={220} onChange={setPendingDownloadSource} options={sourceOptions} />
        <button className="gh sm" disabled={busy} onClick={speedtestSources}><i className="ti ti-bolt" /> 测速</button>
        <button className="pr sm" disabled={busy} onClick={applyDownloadSource} title={`当前已应用：${sourceName(downloadSource)}`}><i className="ti ti-check" /> 应用</button>
      </div>

      <div className={"checkup " + (statusLoading ? "agent" : gitStatus.installed ? "ok" : "bad")}>
        <span className="cnum"><i className={"ti " + (statusLoading ? "ti-loader spin" : gitStatus.installed ? "ti-brand-git" : "ti-download")} style={{ fontSize: 27 }} /></span>
        <div className="ct">
          <div className="t1">Git for Windows · {statusLoading ? "检测中" : gitStatus.installed ? "已就绪" : "未安装"}</div>
          <div className="t2">{statusLoading ? "正在检测 Git 命令、Git Bash 和安全凭据组件…" : gitStatus.installed ? `${gitStatus.version || "Git 可用"} · ${gitStatus.path || ""}` : "安装后可使用账号终端、工程初始化和仓库迁移功能。"}</div>
          {updateInfo && <div className="t2">{updateInfo.has_update ? `「${updateInfo.source_name}」提供 ${updateInfo.latest}，当前 ${updateInfo.current}。` : `当前版本 ${updateInfo.current} 已是「${updateInfo.source_name}」提供的最新版本。`}</div>}
        </div>
        <div className="cacts">
          <button className="gh sm" disabled={busy || statusLoading} onClick={refresh}><i className="ti ti-refresh" /> 刷新状态</button>
          <button className="gh sm" disabled={busy || statusLoading || !gitStatus.installed} onClick={checkUpdate}><i className="ti ti-cloud-search" /> 检查更新</button>
          {!statusLoading && !gitStatus.installed && <button className="pr sm" disabled={busy} onClick={installGit}><i className="ti ti-download" /> 安装 Git</button>}
          {gitStatus.installed && updateInfo?.has_update && <button className="pr sm" disabled={busy} onClick={installGit}><i className="ti ti-download" /> 立即更新</button>}
        </div>
      </div>

      <div className="effbox">
        <div className="eh"><i className="ti ti-target" /> 生效情况 <span className="sub">账号环境相互隔离，不设置全局默认账号</span></div>
        <div className="effrow"><div className="ek"><i className="ti ti-terminal-2" /> Git 命令</div><div className="ev">{statusLoading ? "检测中…" : gitStatus.version || "未检测到"}</div>{!statusLoading && (gitStatus.installed ? <span className="bd g">可用</span> : <span className="bd r">缺失</span>)}</div>
        <div className="effrow"><div className="ek"><i className="ti ti-brand-git" /> Git Bash</div><div className="ev mono">{statusLoading ? "检测中…" : gitStatus.bash_path || "未检测到"}</div>{!statusLoading && (gitStatus.bash_path ? <span className="bd g">可用</span> : <span className="bd n">未安装</span>)}</div>
        <div className="effrow"><div className="ek" title="Git Credential Manager：由 Git for Windows 提供，用于安全保存和调用 GitHub、Gitee 等平台的 HTTPS 账号凭据。"><i className="ti ti-shield-lock" /> GCM <i className="ti ti-help-circle" style={{ fontSize: 13, opacity: 0.6 }} /></div><div className="ev">{statusLoading ? "检测中…" : github.gcm_version ? `版本 ${github.gcm_version} · 管理 HTTPS 账号凭据` : gitStatus.credential_helper || "未检测到"}</div>{!statusLoading && (github.gcm_available ? <span className="bd g">可用</span> : <span className="bd r">缺失</span>)}</div>
      </div>

      <div className="grouphd git-account-heading" style={{ marginTop: 18 }}>
        <span className="gt"><i className="ti ti-users" /> 账号执行环境 <span className="cnt">{accounts.length} 个账号</span></span>
        <div className="ghr">
          <button className="pr sm" disabled={busy || accounts.length < 1} onClick={beginMigration}><i className="ti ti-arrows-transfer-up-down" /> 迁移仓库</button>
          <button className="pr sm" disabled={busy || statusLoading || !github.gcm_available} onClick={() => { setAccountToken(""); setAddProvider("github"); }}><i className="ti ti-brand-github" /> 添加 GitHub</button>
          <button className="pr sm" disabled={busy || statusLoading || !github.gcm_available} onClick={() => { setAccountToken(""); setAddProvider("gitee"); }}><i className="ti ti-letter-g" /> 添加 Gitee</button>
          <button className="pr sm" disabled={busy || statusLoading || !github.gcm_available} onClick={() => { setAccountToken(""); setCustomServiceUrl(""); setCustomServiceName(""); setCustomUsername(""); setAddProvider("custom"); }}><i className="ti ti-server" /> 添加其他账号</button>
        </div>
      </div>
      <div className="seclead">每个账号拥有独立的终端上下文和仓库级提交身份，不会修改其他终端正在使用的账号。</div>

      {statusLoading ? (
        <Loading text="正在读取已管理账号…" />
      ) : accounts.length === 0 ? (
        <div className="empty"><div className="ei"><i className="ti ti-users" /></div><div className="eh">尚未添加代码托管账号</div><div className="ed">添加 GitHub、Gitee 或其他 Git 服务账号后，可打开账号终端、初始化工程并执行仓库迁移。</div></div>
      ) : accounts.map((account) => {
        const expiry = tokenExpiry(account);
        const accountReady = account.authenticated && expiry?.className !== "r";
        const credentialBadge = !account.authenticated
          ? { className: "r", label: "凭据缺失", detail: "请重新添加该平台账号以保存新的访问令牌（token）" }
          : account.token_verified === false
            ? { className: "w", label: "令牌（token）已保存", detail: "该服务未提供可识别的账号验证接口；令牌（token）已保存到 Windows 凭据管理器，将在首次访问仓库时由服务端验证" }
          : expiry || { className: "g", label: "令牌（token）已验证", detail: "访问令牌（token）已保存在 Windows 凭据管理器" };
        return <div className="gitacc git-profile" key={accountKey(account)}>
          {(() => {
            const identity = account.display_name && account.email
              ? `提交身份：${account.display_name} <${account.email}>`
              : "提交身份未配置；初始化工程前需要填写。";
            return (
          <div className="gitacc-main">
            <span className="av st"><i className={"ti " + (account.platform === "github" ? "ti-brand-github" : account.platform === "gitee" ? "ti-letter-g" : "ti-server")} /></span>
            <div className="mt">
              <div className="t">
                <span className="git-account-name">{account.username}</span>
                <span className="bd b" title={[account.base_url, account.provider].filter(Boolean).join(" · ")}>{accountPlatformName(account)}</span>
                <span className={`bd ${credentialBadge.className}`} title={credentialBadge.detail}>{credentialBadge.label}</span>
                <span className="git-identity-inline dim" title={identity}>{identity}</span>
              </div>
            </div>
          </div>
            );
          })()}
          <div className="gitacc-acts">
            <button className="pr sm" disabled={busy || !accountReady || !shells.powershell} title={!accountReady ? credentialBadge.detail : "打开该账号的 PowerShell 终端"} onClick={() => openAccountTerminal(account, "powershell")}><i className="ti ti-terminal-2" /> PS</button>
            <button className="pr sm" disabled={busy || !accountReady || !shells.gitbash} title={!accountReady ? credentialBadge.detail : "打开该账号的 Git Bash 终端"} onClick={() => openAccountTerminal(account, "gitbash")}><i className="ti ti-brand-git" /> Bash</button>
            <button className="pr sm" disabled={busy || !accountReady || !shells.cmd} title={!accountReady ? credentialBadge.detail : "打开该账号的 cmd 终端"} onClick={() => openAccountTerminal(account, "cmd")}><i className="ti ti-terminal" /> cmd</button>
            <button className="gh sm" disabled={busy || !accountReady} title={!accountReady ? credentialBadge.detail : "写入普通终端默认提交身份和该平台默认 HTTPS 账号"} onClick={() => setAccountGlobal(account)}><i className="ti ti-user-check" /> 设为全局</button>
            <button className="gh sm" disabled={busy || !accountReady} title={!accountReady ? credentialBadge.detail : undefined} onClick={() => beginInit(account)}><i className="ti ti-folder-plus" /> 初始化工程</button>
            <button className="gh sm" disabled={busy || !accountReady} title={!accountReady ? credentialBadge.detail : "复制该账号的 Git 操作摘要，方便 AI 按指定账号操作仓库"} onClick={() => copyAccountContext(account)}><i className="ti ti-copy" /> 复制摘要给 AI</button>
            <button className="gh sm" disabled={busy} title="设置提交身份" onClick={() => beginEditIdentity(account)}><i className="ti ti-id" /></button>
            <button className="gh sm danger" disabled={busy} title="移除账号" onClick={() => setRemoveAccount(account)}><i className="ti ti-trash" /></button>
          </div>
        </div>;
      })}

      <div className="grouphd" style={{ marginTop: 18 }}><span className="gt"><i className="ti ti-world-bolt" /> Git 代理 <span className="cnt">Git 全局网络配置</span></span></div>
      <div className="srcrow">
        <span className="av st"><i className="ti ti-world-bolt" /></span>
        <div className="mt"><div className="t">HTTP / HTTPS 代理 {proxyConfigured ? <span className="bd g">已配置</span> : <span className="bd n">未配置</span>}</div><div className="s dim">使用设置页中保存的全局代理地址。</div><div className="s mono">{statusLoading ? "检测中…" : gitStatus.http_proxy || gitStatus.https_proxy || "未配置"}</div></div>
        <button className="pr sm" disabled={!gitStatus.installed || busy} onClick={applyProxy}><i className="ti ti-check" /> 应用</button>
        <button className="gh sm" disabled={!gitStatus.installed || busy || !proxyConfigured} onClick={clearProxy}><i className="ti ti-eraser" /> 清除</button>
      </div>

      {addProvider && (
        <Modal
          title={`添加 ${platformName(addProvider)} 账号`}
          icon={addProvider === "github" ? "ti-brand-github" : addProvider === "gitee" ? "ti-letter-g" : "ti-server"}
          sub={addProvider === "custom" ? "支持 GitLab、Gitea、Forgejo、GitHub Enterprise 及通用 HTTPS Git 服务；令牌（token）仅保存到 Windows 凭据管理器。" : "Stacker 将验证令牌（token）并自动识别账号；令牌（token）仅保存到 Windows 凭据管理器。"}
          onClose={() => { if (!busy) { setAccountToken(""); setCustomServiceUrl(""); setCustomServiceName(""); setCustomUsername(""); setAddProvider(null); } }}
          footer={<>
            <button className="gh sm" disabled={busy} onClick={() => { setAccountToken(""); setCustomServiceUrl(""); setCustomServiceName(""); setCustomUsername(""); setAddProvider(null); }}>取消</button>
            {addProvider !== "custom" && <button className="gh sm" disabled={busy} onClick={() => openUrl(addProvider === "github" ? "https://github.com/settings/tokens" : "https://gitee.com/profile/personal_access_tokens")}><i className="ti ti-external-link" /> 获取令牌（token）</button>}
            <button className="pr sm" disabled={busy || !accountToken.trim() || (addProvider === "custom" && (!customServiceUrl.trim() || !customUsername.trim()))} onClick={saveTokenAccount}><i className="ti ti-shield-check" /> 验证并添加</button>
          </>}
        >
          {addProvider === "custom" && <>
            <div className="field">
              <label>Git 服务地址</label>
              <input className="ip full" autoFocus value={customServiceUrl} placeholder="例如 https://git.example.com" onChange={(event) => setCustomServiceUrl(event.target.value)} />
            </div>
            <div className="git-modal-grid">
              <div className="field"><label>登录账号</label><input className="ip full" value={customUsername} placeholder="令牌（token）所属账号" onChange={(event) => setCustomUsername(event.target.value)} /></div>
              <div className="field"><label>平台名称</label><input className="ip full" value={customServiceName} placeholder="自动识别或自定义平台名称" onChange={(event) => setCustomServiceName(event.target.value)} /></div>
            </div>
          </>}
          <div className="field">
            <label>访问令牌（token）</label>
            <input className="ip full" type="password" autoComplete="new-password" autoFocus={addProvider !== "custom"} value={accountToken} placeholder="粘贴具有仓库读写权限的访问令牌（token）" onChange={(event) => setAccountToken(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") saveTokenAccount(); }} />
          </div>
          <div className="git-token-help"><i className="ti ti-info-circle" /> {addProvider === "custom" ? "可识别的平台会验证账号并读取令牌（token）有效期；通用服务会安全保存凭据，并在首次访问仓库时验证。" : "令牌（token）至少需要仓库读写权限；创建、迁移私有仓库时还需相应的仓库管理权限。"}</div>
        </Modal>
      )}

      {editAccount && (
        <Modal title={`设置 ${editAccount.username} 的提交身份`} icon="ti-id" sub="此身份写入由该账号初始化的仓库，不修改 Git 全局配置。" onClose={() => !busy && setEditAccount(null)} footer={<><button className="gh sm" disabled={busy} onClick={() => setEditAccount(null)}>取消</button><button className="pr sm" disabled={busy || !editName.trim() || !editEmail.trim()} onClick={() => saveIdentity()}><i className="ti ti-check" /> 保存</button></>}>
          <div className="field"><label>提交姓名</label><input className="ip full" autoFocus value={editName} onChange={(event) => setEditName(event.target.value)} /></div>
          <div className="field"><label>提交邮箱</label><input className="ip full" value={editEmail} placeholder="建议使用平台已验证邮箱" onChange={(event) => setEditEmail(event.target.value)} /></div>
        </Modal>
      )}

      {initAccount && (
        <Modal wide title={`使用 ${accountPlatformName(initAccount)} · ${initAccount.username} 初始化工程`} icon="ti-folder-plus" sub="只提交自动创建的 README，不会自动提交目录中的其他文件。" onClose={() => !busy && setInitAccount(null)} footer={<><button className="gh sm" disabled={busy} onClick={() => setInitAccount(null)}>取消</button><button className="pr sm" disabled={busy || !initDirectory || !repositoryName.trim() || !editName.trim() || !editEmail.trim()} onClick={initializeRepository}><i className="ti ti-folder-plus" /> 初始化</button></>}>
          <div className="git-modal-grid">
            <div className="field git-span-2"><label>工程目录</label><div className="row"><input className="ip full" readOnly value={initDirectory} placeholder="选择尚未初始化 Git 的工程目录" /><button className="gh sm" onClick={chooseInitDirectory}><i className="ti ti-folder-open" /> 选择</button></div></div>
            <div className="field"><label>仓库名称</label><input className="ip full" value={repositoryName} onChange={(event) => setRepositoryName(event.target.value)} /></div>
            <div className="field"><label>可见性</label><div className="seg"><button className={!privateRepository ? "on" : ""} onClick={() => setPrivateRepository(false)}>公开</button><button className={privateRepository ? "on" : ""} onClick={() => setPrivateRepository(true)}>私有</button></div></div>
            <div className="field git-span-2"><label>仓库描述</label><input className="ip full" value={description} placeholder="可选" onChange={(event) => setDescription(event.target.value)} /></div>
            <div className="field"><label>提交姓名</label><input className="ip full" value={editName} onChange={(event) => setEditName(event.target.value)} /></div>
            <div className="field"><label>提交邮箱</label><input className="ip full" value={editEmail} onChange={(event) => setEditEmail(event.target.value)} /></div>
            <label className="ck" title={!supportsRemoteRepositoryCreation(initAccount) ? remoteRepositoryCreationHint(initAccount) : undefined}><input type="checkbox" disabled={!supportsRemoteRepositoryCreation(initAccount)} checked={createRemote} onChange={(event) => setCreateRemote(event.target.checked)} /> 同时创建远程仓库</label>
            <label className="ck"><input type="checkbox" checked={createReadme} onChange={(event) => setCreateReadme(event.target.checked)} /> 创建 README、初始提交并推送</label>
            {!createRemote && <div className="field git-span-2"><label>已有远程仓库地址</label><input className="ip full" value={remoteUrl} placeholder={`可选：${initAccount.base_url || (initAccount.platform === "github" ? "https://github.com" : "https://gitee.com")}/${initAccount.username}/repository.git`} onChange={(event) => setRemoteUrl(event.target.value)} /></div>}
          </div>
        </Modal>
      )}

      {migrationOpen && (
        <Modal wide title="迁移仓库" icon="ti-arrows-transfer-up-down" sub="选择源账号与目标账号，Stacker 会根据平台和仓库归属自动选择迁移方式。" onClose={() => !busy && setMigrationOpen(false)} footer={<><button className="gh sm" disabled={busy} onClick={() => setMigrationOpen(false)}>取消</button><button className="pr sm" disabled={busy || !sourceAccount || !targetAccount || !sourceOwner.trim() || !sourceRepository.trim() || !targetRepository.trim()} onClick={migrateRepository}><i className="ti ti-arrows-transfer-up-down" /> 开始迁移</button></>}>
          <div className="git-migration-flow">
            <div className="git-migration-side">
              <div className="git-migration-title"><i className="ti ti-arrow-up-right" /> 源仓库</div>
              <div className="field"><label>源账号</label><Select value={sourceAccount} options={allAccountOptions} onChange={(value) => { setSourceAccount(value); const account = selectedAccount(value); if (account) setSourceOwner(account.username); }} /></div>
              <div className="field"><label>仓库所有者</label><input className="ip full" value={sourceOwner} placeholder="个人账号或组织名称" onChange={(event) => setSourceOwner(event.target.value)} /></div>
              <div className="field"><label>源仓库名称</label><input className="ip full" value={sourceRepository} placeholder="例如 old-project" onChange={(event) => setSourceRepository(event.target.value)} /></div>
            </div>
            <div className="git-migration-arrow"><i className="ti ti-arrow-right" /></div>
            <div className="git-migration-side">
              <div className="git-migration-title"><i className="ti ti-arrow-down-left" /> 目标仓库</div>
              <div className="field"><label>目标账号</label><Select value={targetAccount} options={allAccountOptions} onChange={setTargetAccount} /></div>
              <div className="field"><label>目标仓库名称</label><input className="ip full" value={targetRepository} placeholder="例如 new-project" onChange={(event) => setTargetRepository(event.target.value)} /></div>
              <div className="field"><label>新建仓库可见性</label><div className="seg"><button className={!targetPrivate ? "on" : ""} onClick={() => setTargetPrivate(false)}>公开</button><button className={targetPrivate ? "on" : ""} onClick={() => setTargetPrivate(true)}>私有</button></div></div>
            </div>
          </div>
          <label className="ck"><input type="checkbox" checked={includeLfs} onChange={(event) => setIncludeLfs(event.target.checked)} /> 迁移 Git LFS 对象（使用镜像迁移且本机已安装 Git LFS 时生效）</label>
          <div className="banner amber"><i className="ti ti-info-circle lead" /><div className="bt">GitHub 跨所有者时使用原生转移并保留平台数据；同账号改仓库名、Gitee 或跨平台场景会创建目标仓库并镜像复制，源仓库不会自动删除。</div></div>
        </Modal>
      )}

      {removeAccount && (
        <ConfirmModal title="移除账号执行环境" icon="ti-trash" danger busy={busy} message={<>将从本机移除 <b>{accountPlatformName(removeAccount)} · {removeAccount.username}</b> 的安全凭据。该操作不会删除远程仓库。</>} confirmLabel={busy ? "移除中…" : "确认移除"} onConfirm={confirmRemoveAccount} onClose={() => setRemoveAccount(null)} />
      )}
    </>
  );
}
