import { useEffect, useState } from "react";
import { invoke } from "../invoke";
import { open } from "@tauri-apps/plugin-dialog";
import { SourcesPanel } from "../SourcesPanel";
import { VersionManager } from "../VersionManager";
import { Select } from "../Select";
import { useBusy, useToast } from "../ui";

const GRADLE_DOWNLOAD_SOURCES = [
  { id: "official", name: "官方 Gradle", host: "services.gradle.org", url: "https://services.gradle.org/distributions" },
  { id: "tencent", name: "腾讯云", host: "mirrors.cloud.tencent.com", url: "https://mirrors.cloud.tencent.com/gradle" },
  { id: "aliyun", name: "阿里云", host: "mirrors.aliyun.com", url: "https://mirrors.aliyun.com/gradle/distributions" },
  { id: "huawei", name: "华为云", host: "repo.huaweicloud.com", url: "https://repo.huaweicloud.com/gradle" },
];

function gradleDownloadUrl(source: { id: string; url: string }, version: string) {
  const base = source.url.replace(/\/$/, "");
  return source.id === "aliyun" || base.includes("mirrors.aliyun.com/gradle")
    ? `${base}/v${version}/gradle-${version}-bin.zip`
    : `${base}/gradle-${version}-bin.zip`;
}

const GRADLE_WRAPPER_SOURCES = [
  { id: "official", name: "官方 Gradle", host: "services.gradle.org" },
  { id: "tencent", name: "腾讯云", host: "mirrors.cloud.tencent.com" },
  { id: "aliyun", name: "阿里云", host: "mirrors.aliyun.com" },
  { id: "huawei", name: "华为云", host: "repo.huaweicloud.com" },
];

type WrapperState = {
  path: string;
  exists: boolean;
  distribution_url: string;
  version: string;
  package_type: string;
  source_id: string;
  source_label: string;
};

const WRAPPER_PATH_KEY = "stacker.gradle.wrapperPath";
const WRAPPER_SOURCE_KEY = "stacker.gradle.wrapperSource";

function fileName(path: string) {
  return path.split(/[\\/]/).pop() || path;
}

type SourcePing = { host: string; ms: number | null };

function GradleWrapperPanel() {
  const toast = useToast();
  const runBusy = useBusy();
  const [path, setPath] = useState(() => localStorage.getItem(WRAPPER_PATH_KEY) ?? "");
  const [state, setState] = useState<WrapperState | null>(null);
  const [scanRows, setScanRows] = useState<WrapperState[]>([]);
  const [busy, setBusy] = useState(false);
  const [wrapperSource, setWrapperSource] = useState(() => {
    const saved = localStorage.getItem(WRAPPER_SOURCE_KEY);
    return saved && GRADLE_WRAPPER_SOURCES.some((s) => s.id === saved) ? saved : "official";
  });
  const [pendingSource, setPendingSource] = useState(wrapperSource);
  const [pings, setPings] = useState<Record<string, number | null>>({});
  const [testing, setTesting] = useState(false);

  async function load(nextPath = path) {
    if (!nextPath.trim()) {
      setState(null);
      return;
    }
    const s = await invoke<WrapperState>("gradle_wrapper_state", { path: nextPath });
    setState(s);
    if (s.source_id && GRADLE_WRAPPER_SOURCES.some((src) => src.id === s.source_id)) {
      setWrapperSource(s.source_id);
      setPendingSource(s.source_id);
      localStorage.setItem(WRAPPER_SOURCE_KEY, s.source_id);
    }
  }

  useEffect(() => {
    load().catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function chooseFile() {
    const file = await open({
      directory: false,
      multiple: false,
      filters: [{ name: "gradle-wrapper.properties", extensions: ["properties"] }],
    });
    if (!file || typeof file !== "string") return;
    if (fileName(file).toLowerCase() !== "gradle-wrapper.properties") {
      toast("请选择 gradle-wrapper.properties 文件", "err");
      return;
    }
    localStorage.setItem(WRAPPER_PATH_KEY, file);
    setPath(file);
    await load(file);
  }

  async function scanDir() {
    const dir = await open({ directory: true, multiple: false });
    if (!dir || typeof dir !== "string") return;
    setBusy(true);
    try {
      const rows = await runBusy({
        title: "扫描 Gradle Wrapper",
        message: "正在查找所选目录下的 gradle-wrapper.properties 文件。",
      }, () => invoke<WrapperState[]>("gradle_wrapper_scan", { root: dir }));
      setScanRows(rows);
      if (rows.length === 1) {
        localStorage.setItem(WRAPPER_PATH_KEY, rows[0].path);
        setPath(rows[0].path);
        setState(rows[0]);
      }
      toast(rows.length ? `扫描完成，发现 ${rows.length} 个 Wrapper 配置` : "未发现 Gradle Wrapper 配置", rows.length ? "ok" : "info");
    } catch (e) {
      toast("扫描 Gradle Wrapper 失败。请缩小目录范围后重试。原因：" + e, "err");
    } finally {
      setBusy(false);
    }
  }

  async function applyWrapper(source = pendingSource) {
    if (!path.trim()) {
      toast("请先选择 gradle-wrapper.properties 文件", "info");
      return;
    }
    const applyingName = GRADLE_WRAPPER_SOURCES.find((s) => s.id === source)?.name ?? source;
    setBusy(true);
    try {
      const next = await runBusy({
        title: "应用 Gradle Wrapper 下载源",
        message: `正在把项目 Wrapper 的 distributionUrl 切换到「${applyingName}」。`,
      }, () => invoke<WrapperState>("gradle_wrapper_apply", { path, sourceId: source }));
      setState(next);
      setWrapperSource(source);
      setPendingSource(source);
      localStorage.setItem(WRAPPER_SOURCE_KEY, source);
      toast(source === "official" ? "已清除 Gradle Wrapper 下载源配置，恢复官方地址" : `已应用 Gradle Wrapper 下载源：${applyingName}`, "ok");
    } catch (e) {
      toast("应用 Gradle Wrapper 下载源失败。请确认文件可写后重试。原因：" + e, "err");
    } finally {
      setBusy(false);
    }
  }

  function pick(row: WrapperState) {
    localStorage.setItem(WRAPPER_PATH_KEY, row.path);
    setPath(row.path);
    setState(row);
    if (GRADLE_WRAPPER_SOURCES.some((src) => src.id === row.source_id)) {
      setWrapperSource(row.source_id);
      setPendingSource(row.source_id);
      localStorage.setItem(WRAPPER_SOURCE_KEY, row.source_id);
    }
  }

  async function speedtestSources() {
    const hosts = [...new Set(GRADLE_WRAPPER_SOURCES.map((s) => s.host).filter(Boolean))];
    setTesting(true);
    try {
      const rows = await runBusy({
        title: "Gradle Wrapper 下载源测速",
        message: "正在并行测试各下载源连接延迟；单个主机 1500ms 无响应算超时。",
      }, () => invoke<SourcePing[]>("speedtest_hosts", { hosts }));
      const byHost: Record<string, number | null> = {};
      rows.forEach((r) => { byHost[r.host] = r.ms; });
      const bySource: Record<string, number | null> = {};
      GRADLE_WRAPPER_SOURCES.forEach((s) => { bySource[s.id] = byHost[s.host] ?? null; });
      setPings(bySource);
      const fastest = GRADLE_WRAPPER_SOURCES
        .map((s) => ({ ...s, ms: bySource[s.id] }))
        .filter((s): s is typeof GRADLE_WRAPPER_SOURCES[number] & { ms: number } => typeof s.ms === "number")
        .sort((a, b) => a.ms - b.ms)[0];
      if (fastest) {
        setPendingSource(fastest.id);
        toast(fastest.id === wrapperSource
          ? `测速完成，${fastest.name} 已是当前 Wrapper 下载源`
          : `测速完成，已预选 ${fastest.name}，点击「应用」后生效`, "ok");
      } else {
        toast("Wrapper 下载源测速均超时，保留当前选择", "info");
      }
    } catch (e) {
      toast("Wrapper 下载源测速失败。请检查网络连接后重试。原因：" + e, "err");
    } finally {
      setTesting(false);
    }
  }

  function fastestSource() {
    return Object.entries(pings)
      .filter(([, ms]) => typeof ms === "number")
      .sort((a, b) => (a[1] as number) - (b[1] as number))[0]?.[0] ?? null;
  }

  function sourceOptions() {
    const fast = fastestSource();
    return GRADLE_WRAPPER_SOURCES.map((s) => {
      const ms = pings[s.id];
      const suffix = !(s.id in pings)
        ? ""
        : ms === null ? " · 超时" : ` · ${ms}ms${s.id === fast ? " · 最快" : ""}`;
      return { value: s.id, label: `${s.name}${suffix}` };
    });
  }

  return (
    <>
      <div className="grouphd gradle-wrapper-heading" style={{ marginTop: 18 }}>
        <span className="gt"><i className="ti ti-route" /> Wrapper 下载源 <span className="cnt">gradle-wrapper.properties</span></span>
        <span className="hint2">用于项目首次同步时下载 Gradle 发行包，Android Studio 会读取这里的地址</span>
      </div>
      <div className="srcrow gradle-wrapper-row">
        <span className="av gr"><i className="ti ti-route" /></span>
        <div className="mt">
          <div className="t">项目 Gradle Wrapper {state?.source_id && state.source_id !== "missing" ? <span className={state.source_id === "custom" ? "bd w" : "bd g"}>{state.source_label}</span> : <span className="bd off">未选择</span>}</div>
          <div className="s dim" title={state?.distribution_url || "选择项目 gradle-wrapper.properties 后，可把 distributionUrl 切换到当前 Gradle 下载源。"}>
            {state?.distribution_url ? `当前：${state.source_label} · Gradle ${state.version || "未知版本"}` : "选择项目 wrapper 文件后，可切换 Gradle 发行包下载源。"}
          </div>
          <div className={"s" + (path ? " mono" : " dim")} title={path || "未选择"}>{path || "未选择"}</div>
        </div>
        <div className="gradle-wrapper-actions">
          <button className="gh sm" disabled={busy} onClick={scanDir}><i className="ti ti-search" /> 扫描目录</button>
          <button className="gh sm" disabled={busy} onClick={chooseFile}><i className="ti ti-file-search" /> 选择文件</button>
          <Select value={pendingSource} width={190} onChange={setPendingSource} options={sourceOptions()} />
          <button className="gh sm" disabled={busy || testing} onClick={speedtestSources}>
            <i className={"ti " + (testing ? "ti-loader spin" : "ti-bolt")} /> {testing ? "测速中…" : "测速"}
          </button>
          <button className="pr sm" disabled={busy || !path.trim()} onClick={() => applyWrapper()}><i className="ti ti-check" /> 应用</button>
          <button className="gh sm" disabled={busy || !path.trim()} onClick={() => applyWrapper("official")}><i className="ti ti-eraser" /> 清除</button>
        </div>
      </div>
      {scanRows.length > 1 && (
        <div className="srcrow" style={{ alignItems: "center" }}>
          <span className="av gr"><i className="ti ti-list-search" /></span>
          <div className="mt">
            <div className="t">扫描结果</div>
            <div className="s dim">选择要配置的项目 Wrapper。</div>
          </div>
          <Select value={path} width={420} onChange={(v) => {
            const row = scanRows.find((r) => r.path === v);
            if (row) pick(row);
          }} options={scanRows.map((r) => ({ value: r.path, label: `${r.version || "未知版本"} · ${r.source_label} · ${r.path}` }))} />
        </div>
      )}
    </>
  );
}

export default function Gradle() {
  const [srcKey, setSrcKey] = useState(0);
  return (
    <>
      <VersionManager kind="gradle" icon="ti-box" cmd="gradle" envvar="GRADLE_HOME" onChanged={() => setSrcKey((k) => k + 1)}
        download={{
          title: "下载 Gradle",
          subdir: "gradle",
          folderName: (v) => `gradle-${v}`,
          defaultSource: "official",
          sourceToolId: "gradle-runtime",
          sources: GRADLE_DOWNLOAD_SOURCES,
          urlFor: gradleDownloadUrl,
          note: "版本列表按当前下载源实际存在的发行包生成。",
          versionsCmd: "gradle_versions",
          staticVersions: ["8.12", "8.10", "7.6.4"],
        }} />

      <GradleWrapperPanel />

      <div className="grouphd" style={{ marginTop: 18 }}>
        <span className="gt"><i className="ti ti-world-download" /> 仓库镜像 <span className="cnt">init.gradle</span></span>
        <span className="hint2">配置当前用户 init.gradle；特殊初始化脚本可手动选择后单独处理</span>
      </div>
      <SourcesPanel toolIds={["gradle"]} refresh={srcKey} />

      <div className="callout"><i className="ti ti-info-circle" /><div>仓库镜像会写入当前用户的 <span className="code">init.gradle</span>，并保留官方仓库作为回退。需要接入私有仓库时，可在「设置 → 源管理」添加自定义源。</div></div>
    </>
  );
}
