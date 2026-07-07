import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useBusy, useToast } from "./ui";
import { Select } from "./Select";

type Mirror = { id: string; name: string; url: string; host: string };
type ToolState = {
  id: string;
  name: string;
  icon: string;
  config: string;
  installed: boolean;
  current: string | null;
  current_label: string;
  mirrors: Mirror[];
};
type PipScope = {
  id: string;
  kind: string;
  name: string;
  path: string;
  exists: boolean;
  configured: boolean;
  requires_admin: boolean;
  current: string | null;
  current_label: string;
  effective: string | null;
  effective_label: string;
  overridden_by: string | null;
};
type PipState = {
  mirrors: Mirror[];
  scopes: PipScope[];
  env_overrides: string[];
  effective: string | null;
  effective_label: string;
};
type ProxyStatus = {
  host: string;
  port: number;
  detected_port: number | null;
};
type SourceFileState = {
  current: string | null;
  current_label: string;
};

const ICON: Record<string, string> = {
  pip: "ti-brand-python",
  conda: "ti-package",
  npm: "ti-brand-npm",
  yarn: "ti-package",
  go: "ti-world-download",
  maven: "ti-world-download",
  gradle: "ti-world-download",
  cargo: "ti-package",
};
const AV: Record<string, string> = {
  pip: "py",
  conda: "cd",
  npm: "npm",
  yarn: "yn",
  go: "go",
  maven: "mv2",
  gradle: "gr",
  cargo: "rs",
};
const CUSTOM_PIP_PATH_KEY = "stacker.pip.customPath";
const CUSTOM_CONFIG_PATH_KEYS: Record<string, string> = {
  maven: "stacker.maven.customSettingsXml",
  gradle: "stacker.gradle.customInitGradle",
};

function badge(t: ToolState) {
  if (!t.installed) return <span className="bd off">未检测到</span>;
  if (t.current === "official") return <span className="bd n">官方</span>;
  if (t.current) return <span className="bd g">已配置</span>;
  return <span className="bd w">未识别</span>;
}

function customSourceBadge(path: string, state?: SourceFileState) {
  if (!path.trim()) return <span className="bd off">未选择</span>;
  if (state?.current === "official") return <span className="bd n">官方</span>;
  if (state?.current) return <span className="bd g">已配置</span>;
  return <span className="bd w">未识别</span>;
}

function pipKey(scope: PipScope) {
  return `${scope.kind}:${scope.path}`;
}

function customConfigKey(toolId: string, path: string) {
  return `custom-config:${toolId}:${path}`;
}

function rowProxyKey(toolId: string, path?: string) {
  return path ? `${toolId}:${path}` : toolId;
}

function customRowProxyKey(toolId: string, path: string) {
  return `${toolId}:custom:${path || "__unselected__"}`;
}

function customConfigName(toolId: string) {
  return toolId === "maven" ? "自选 settings.xml" : "自选 init.gradle";
}

function customConfigHint(toolId: string) {
  return toolId === "maven"
    ? "选择 settings.xml 后可单独配置 Maven 仓库镜像。"
    : "选择 init.gradle 后可单独配置 Gradle 仓库镜像。";
}

function sourceActionName(id: string) {
  return id === "maven" || id === "gradle" ? "仓库镜像" : "源";
}

function pipBadge(scope: PipScope) {
  if (scope.kind === "custom" && !scope.path) return <span className="bd off">未选择</span>;
  if (scope.configured && scope.overridden_by) {
    return <><span className="bd g">已配置</span><span className="bd w">被{scope.overridden_by}覆盖</span></>;
  }
  if (scope.overridden_by) return <span className="bd w">被{scope.overridden_by}覆盖</span>;
  if (scope.configured) return <span className="bd g">已配置</span>;
  return <span className="bd n">继承</span>;
}

function missingText(t: ToolState) {
  if (t.id === "npm") return "未检测到 npm。安装并设置默认 Node 版本后可配置镜像。";
  if (t.id === "yarn") return "未检测到 Yarn。Yarn 需单独安装或启用 Corepack 后配置镜像。";
  return "未检测到，无法配置镜像";
}

export function SourcesPanel({ toolIds, refresh }: { toolIds: string[]; refresh?: number }) {
  const toast = useToast();
  const runBusy = useBusy();
  const [tools, setTools] = useState<ToolState[] | null>(null);
  const [pipState, setPipState] = useState<PipState | null>(null);
  const [customPipPath, setCustomPipPath] = useState(() => localStorage.getItem(CUSTOM_PIP_PATH_KEY) ?? "");
  const [customConfigPaths, setCustomConfigPaths] = useState<Record<string, string>>(() => {
    const out: Record<string, string> = {};
    for (const [id, key] of Object.entries(CUSTOM_CONFIG_PATH_KEYS)) {
      out[id] = localStorage.getItem(key) ?? "";
    }
    return out;
  });
  const [err, setErr] = useState(false);
  const [sel, setSel] = useState<Record<string, string>>({});
  const [pipSel, setPipSel] = useState<Record<string, string>>({});
  const [sourceScopes, setSourceScopes] = useState<Record<string, "user" | "system">>({ go: "user" });
  const [sourceProxy, setSourceProxy] = useState<Record<string, boolean>>({});
  const [customConfigState, setCustomConfigState] = useState<Record<string, SourceFileState>>({});
  const [proxyAddr, setProxyAddr] = useState<{ host: string; port: number }>({ host: "127.0.0.1", port: 7890 });
  const [busy, setBusy] = useState("");
  const [pings, setPings] = useState<Record<string, number | null>>({});
  const [testing, setTesting] = useState(false);

  async function load(nextCustomPipPath = customPipPath) {
    const all = await invoke<ToolState[]>("list_sources");
    const mine = toolIds.map((id) => all.find((t) => t.id === id)).filter(Boolean) as ToolState[];
    setTools(mine);
    if (mine.some((t) => t.id === "maven" || t.id === "gradle")) {
      invoke<ProxyStatus>("proxy_status").then((ps) => {
        setProxyAddr({
          host: ps.host || "127.0.0.1",
          port: ps.port || ps.detected_port || 7890,
        });
      }).catch(() => {});
      const rows: Array<{ toolId: string; path?: string }> = [];
      mine.forEach((t) => {
        if (t.id === "maven" || t.id === "gradle") {
          rows.push({ toolId: t.id });
          const customPath = customConfigPaths[t.id]?.trim();
          if (customPath) rows.push({ toolId: t.id, path: customPath });
        }
      });
      Promise.all(rows.map(async (row) => {
        const enabled = await invoke<boolean>("source_proxy_state", { toolId: row.toolId, path: row.path || null });
        return [row.path ? customRowProxyKey(row.toolId, row.path) : rowProxyKey(row.toolId), enabled] as const;
      })).then((items) => {
        setSourceProxy((s) => {
          const n = { ...s };
          items.forEach(([k, v]) => { n[k] = v; });
          return n;
        });
      }).catch(() => {});
      const customRows = rows.filter((row) => row.path);
      Promise.all(customRows.map(async (row) => {
        const state = await invoke<SourceFileState>("source_file_state", { toolId: row.toolId, path: row.path });
        return [customConfigKey(row.toolId, row.path || ""), state] as const;
      })).then((items) => {
        setCustomConfigState((s) => {
          const n = { ...s };
          items.forEach(([k, v]) => { n[k] = v; });
          return n;
        });
      }).catch(() => {});
    }
    setSel((s) => {
      const n = { ...s };
      const warned: string[] = [];
      mine.forEach((x) => {
        const fallback = x.mirrors.find((m) => m.id === "official")?.id ?? x.mirrors[0]?.id ?? "";
        const picked = n[x.id] || x.current || fallback;
        if (picked && !x.mirrors.some((m) => m.id === picked)) {
          n[x.id] = fallback;
          warned.push(`${x.name} 当前选择的源已不在清单中，页面已恢复为官方源；点击「应用」后写入。`);
        } else if (!n[x.id]) {
          n[x.id] = picked;
        }
        if (!x.current && x.current_label === "未识别" && fallback) {
          n[x.id] = fallback;
          warned.push(`${x.name} 当前配置不在源清单中，页面已恢复为官方源；点击「应用」后写入。`);
        }
      });
      warned.forEach((msg) => toast(msg, "info"));
      return n;
    });

    if (toolIds.includes("pip")) {
      const ps = await invoke<PipState>("pip_config_state", { customPath: nextCustomPipPath || null });
      setPipState(ps);
      setPipSel((s) => {
        const n = { ...s };
        ps.scopes.forEach((scope) => {
          const k = pipKey(scope);
          if (!n[k]) n[k] = scope.current ?? scope.effective ?? "official";
        });
        return n;
      });
    } else {
      setPipState(null);
    }
  }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { load().catch(() => setErr(true)); }, [refresh]);

  async function apply(id: string) {
    setBusy(id);
    try {
      if (id === "go") {
        await invoke("apply_source_scoped", { toolId: id, mirrorId: sel[id], scope: sourceScopes.go ?? "user" });
      } else if (id === "maven" || id === "gradle") {
        await invoke("apply_source", {
          toolId: id,
          mirrorId: sel[id],
          proxyEnabled: !!sourceProxy[rowProxyKey(id)],
          proxyHost: proxyAddr.host,
          proxyPort: proxyAddr.port,
        });
      } else {
        await invoke("apply_source", { toolId: id, mirrorId: sel[id] });
      }
      await load();
      toast(id === "go"
        ? `已应用 Go 模块代理（${(sourceScopes.go ?? "user") === "system" ? "系统级" : "用户级"}）`
        : id === "maven" || id === "gradle"
          ? `已应用 ${id} 仓库镜像${sourceProxy[rowProxyKey(id)] ? "并启用代理" : "并关闭代理"}`
          : "已应用 " + id + " 源", "ok");
    } catch (e) {
      toast(`应用 ${id} 源失败。请确认配置文件或环境变量可写后重试。原因：` + e, "err");
    } finally {
      setBusy("");
    }
  }

  async function clearToolConfig(t: ToolState) {
    const key = "clear:" + t.id;
    setBusy(key);
    try {
      await runBusy({
        title: `清除 ${t.name} ${sourceActionName(t.id)}`,
        message: `正在清除当前用户配置中由 Stacker 写入的 ${sourceActionName(t.id)}和代理配置`,
      }, () => invoke("apply_source", {
        toolId: t.id,
        mirrorId: "official",
        proxyEnabled: false,
        proxyHost: proxyAddr.host,
        proxyPort: proxyAddr.port,
      }));
      setSourceProxy((s) => ({ ...s, [rowProxyKey(t.id)]: false }));
      setSel((s) => ({ ...s, [t.id]: "official" }));
      await load();
      toast(`已清除 ${t.name} ${sourceActionName(t.id)}配置`, "ok");
    } catch (e) {
      toast(`清除 ${t.name} ${sourceActionName(t.id)}失败。请确认配置文件可写后重试。原因：` + e, "err");
    } finally {
      setBusy("");
    }
  }

  async function applyPip(scope: PipScope) {
    if (scope.kind === "custom" && !scope.path) {
      toast("请先选择 pip.ini 文件", "info");
      return;
    }
    const k = pipKey(scope);
    setBusy("pip:" + k);
    try {
      await runBusy({
        title: "应用 pip 源",
        message: `正在写入 ${scope.path}`,
      }, () => invoke("pip_apply_source", { scope: scope.kind, path: scope.path || null, mirrorId: pipSel[k] }));
      await load();
      toast("已应用 pip 源配置", "ok");
    } catch (e) {
      toast("应用 pip 源失败。请确认配置文件可写后重试。原因：" + e, "err");
    } finally {
      setBusy("");
    }
  }

  async function clearPip(scope: PipScope) {
    if (scope.kind === "custom" && !scope.path) {
      toast("请先选择 pip.ini 文件", "info");
      return;
    }
    const k = pipKey(scope);
    setBusy("pip-clear:" + k);
    try {
      await runBusy({
        title: "清除 pip 源",
        message: `正在清除 ${scope.path} 中的 index-url / trusted-host`,
      }, () => invoke("pip_clear_source", { scope: scope.kind, path: scope.path || null }));
      await load();
      toast("已清除 pip 源配置", "ok");
    } catch (e) {
      toast("清除 pip 源失败。请确认配置文件可写后重试。原因：" + e, "err");
    } finally {
      setBusy("");
    }
  }

  async function speedtest() {
    if (!tools) return;
    const hosts = [...new Set(tools.flatMap((t) => t.mirrors.map((m) => m.host)).filter(Boolean))];
    if (hosts.length === 0) {
      toast("没有可测速的镜像主机", "info");
      return;
    }
    setTesting(true);
    try {
      const res = await runBusy(
        { title: "包源测速", message: "正在并行测试各镜像主机连接延迟，单个主机 1500ms 无响应算超时。" },
        () => invoke<{ host: string; ms: number | null }[]>("speedtest_hosts", { hosts }),
      );
      const map: Record<string, number | null> = {};
      res.forEach((r) => { map[r.host] = r.ms; });
      setPings(map);
      setSel((s) => {
        const n = { ...s };
        tools.forEach((t) => {
          const ranked = t.mirrors
            .filter((m) => m.host && typeof map[m.host] === "number")
            .sort((a, b) => (map[a.host] as number) - (map[b.host] as number));
          if (ranked[0]) {
            n[t.id] = ranked[0].id;
            if (CUSTOM_CONFIG_PATH_KEYS[t.id]) {
              n[customConfigKey(t.id, customConfigPaths[t.id] ?? "")] = ranked[0].id;
              n[customConfigKey(t.id, "")] = ranked[0].id;
            }
          }
        });
        return n;
      });
      if (pipState) {
        const ranked = pipState.mirrors
          .filter((m) => m.host && typeof map[m.host] === "number")
          .sort((a, b) => (map[a.host] as number) - (map[b.host] as number));
        if (ranked[0]) {
          setPipSel((s) => {
            const n = { ...s };
            pipState.scopes.forEach((scope) => { n[pipKey(scope)] = ranked[0].id; });
            n[`custom:${customPipPath}`] = ranked[0].id;
            n["custom:"] = ranked[0].id;
            return n;
          });
        }
      }
      toast("测速完成，已预选更快镜像，点击「应用」后生效", "ok");
    } catch (e) {
      toast("包源测速失败。请检查网络连接后重试。原因：" + e, "err");
    } finally {
      setTesting(false);
    }
  }

  async function chooseCustomPipIni() {
    const file = await open({
      directory: false,
      multiple: false,
      filters: [{ name: "pip.ini", extensions: ["ini"] }],
    });
    if (!file || typeof file !== "string") return;
    const name = file.split(/[\\/]/).pop()?.toLowerCase();
    if (name !== "pip.ini") {
      toast("请选择名为 pip.ini 的文件", "err");
      return;
    }
    localStorage.setItem(CUSTOM_PIP_PATH_KEY, file);
    setPipSel((s) => {
      const n = { ...s };
      const prev = n[`custom:${customPipPath}`] ?? n["custom:"];
      if (prev && !n[`custom:${file}`]) n[`custom:${file}`] = prev;
      return n;
    });
    setCustomPipPath(file);
    await load(file);
  }

  async function chooseCustomConfig(toolId: string) {
    const file = await open({
      directory: false,
      multiple: false,
      filters: toolId === "maven"
        ? [{ name: "settings.xml", extensions: ["xml"] }]
        : [{ name: "init.gradle", extensions: ["gradle"] }],
    });
    if (!file || typeof file !== "string") return;
    const name = file.split(/[\\/]/).pop()?.toLowerCase();
    const expected = toolId === "maven" ? "settings.xml" : "init.gradle";
    if (name !== expected) {
      toast(`请选择名为 ${expected} 的配置文件`, "err");
      return;
    }
    const storageKey = CUSTOM_CONFIG_PATH_KEYS[toolId];
    if (storageKey) localStorage.setItem(storageKey, file);
    setSel((s) => {
      const n = { ...s };
      const prev = n[customConfigKey(toolId, customConfigPaths[toolId] ?? "")] ?? n[customConfigKey(toolId, "")] ?? n[toolId];
      if (prev) n[customConfigKey(toolId, file)] = prev;
      return n;
    });
    setCustomConfigPaths((s) => ({ ...s, [toolId]: file }));
    invoke<boolean>("source_proxy_state", { toolId, path: file }).then((enabled) => {
      setSourceProxy((s) => ({ ...s, [customRowProxyKey(toolId, file)]: enabled }));
    }).catch(() => {});
    invoke<SourceFileState>("source_file_state", { toolId, path: file }).then((state) => {
      setCustomConfigState((s) => ({ ...s, [customConfigKey(toolId, file)]: state }));
    }).catch(() => {});
  }

  async function applyCustomConfig(t: ToolState) {
    const path = customConfigPaths[t.id] ?? "";
    if (!path.trim()) {
      toast(`请先选择 ${t.id === "maven" ? "settings.xml" : "init.gradle"} 文件`, "info");
      return;
    }
    const key = customConfigKey(t.id, path);
    setBusy(key);
    try {
      await runBusy({
        title: `应用 ${t.name} ${sourceActionName(t.id)}`,
        message: `正在写入 ${path}`,
      }, () => invoke("apply_source_file", {
        toolId: t.id,
        path,
        mirrorId: sel[key] ?? sel[t.id],
        proxyEnabled: !!sourceProxy[customRowProxyKey(t.id, path)],
        proxyHost: proxyAddr.host,
        proxyPort: proxyAddr.port,
      }));
      setCustomConfigState((s) => ({
        ...s,
        [key]: {
          current: sel[key] ?? sel[t.id] ?? null,
          current_label: t.mirrors.find((m) => m.id === (sel[key] ?? sel[t.id]))?.name ?? "未识别",
        },
      }));
      toast(`${t.name} 自选配置文件已更新${sourceProxy[customRowProxyKey(t.id, path)] ? "，代理已启用" : "，代理已关闭"}`, "ok");
    } catch (e) {
      toast(`应用 ${t.name} ${sourceActionName(t.id)}失败。请确认配置文件可写后重试。原因：` + e, "err");
    } finally {
      setBusy("");
    }
  }

  async function clearCustomConfig(t: ToolState) {
    const path = customConfigPaths[t.id] ?? "";
    if (!path.trim()) {
      toast(`请先选择 ${t.id === "maven" ? "settings.xml" : "init.gradle"} 文件`, "info");
      return;
    }
    const key = "clear:" + customConfigKey(t.id, path);
    setBusy(key);
    try {
      await runBusy({
        title: `清除 ${t.name} ${sourceActionName(t.id)}`,
        message: `正在清除 ${path} 中由 Stacker 写入的 ${sourceActionName(t.id)}配置`,
      }, () => invoke("clear_source_file", { toolId: t.id, path }));
      setSourceProxy((s) => ({ ...s, [customRowProxyKey(t.id, path)]: false }));
      setCustomConfigState((s) => ({
        ...s,
        [customConfigKey(t.id, path)]: { current: "official", current_label: "官方" },
      }));
      toast(`${t.name} 自选配置文件已清除`, "ok");
    } catch (e) {
      toast(`清除 ${t.name} ${sourceActionName(t.id)}失败。请确认配置文件可写后重试。原因：` + e, "err");
    } finally {
      setBusy("");
    }
  }

  function fastestOf(mirrors: Mirror[]): string | null {
    let best: { id: string; ms: number } | null = null;
    for (const m of mirrors) {
      const ms = m.host ? pings[m.host] : undefined;
      if (typeof ms === "number" && (!best || ms < best.ms)) best = { id: m.id, ms };
    }
    return best?.id ?? null;
  }

  function pingLabel(host: string): string {
    if (!host || !(host in pings)) return "";
    const ms = pings[host];
    return ms === null ? " · 超时" : ` · ${ms}ms`;
  }

  function mirrorOptions(mirrors: Mirror[], fast: string | null) {
    return mirrors.map((m) => ({
      value: m.id,
      label: `${m.name}${pingLabel(m.host)}${m.id === fast ? " · 最快" : ""}`,
    }));
  }

  if (err) return <div className="banner gray"><i className="ti ti-plug-x lead" /><div className="bt">读取源状态失败（请在 Tauri 应用内运行，浏览器预览没有后端）。</div></div>;
  if (!tools) return <div className="srcrow" style={{ justifyContent: "center", color: "var(--mut)" }}>读取源…</div>;

  const anyInstalled = tools.some((t) => t.installed);
  const hasJvmProxyTools = tools.some((t) => t.id === "maven" || t.id === "gradle");

  function renderPipRows(t: ToolState) {
    if (!t.installed) {
      return (
        <div className="srcrow" key="pip-missing">
          <span className="av py"><i className="ti ti-brand-python" /></span>
          <div className="mt">
            <div className="t">{t.name} {badge(t)}</div>
            <div className="s dim" title="未检测到 pip，无法配置 pip 镜像。">未检测到 pip，无法配置镜像。</div>
          </div>
        </div>
      );
    }
    if (!pipState) {
      return <div className="srcrow" key="pip-loading" style={{ justifyContent: "center", color: "var(--mut)" }}>读取 pip 配置…</div>;
    }
    const fast = fastestOf(pipState.mirrors);
    const scopes = [...pipState.scopes];
    if (!scopes.some((scope) => scope.kind === "custom")) {
      scopes.push({
        id: "custom",
        kind: "custom",
        name: "自选 pip.ini",
        path: customPipPath,
        exists: false,
        configured: false,
        requires_admin: false,
        current: null,
        current_label: customPipPath ? "未识别" : "未选择",
        effective: null,
        effective_label: customPipPath ? "未识别" : "未选择",
        overridden_by: null,
      });
    }
    return (
      <div key="pip-scopes" style={{ display: "contents" }}>
        {pipState.env_overrides.length > 0 && (
          <div className="banner gray">
            <i className="ti ti-alert-triangle lead" />
            <div className="bt">检测到 pip 环境变量覆盖：{pipState.env_overrides.join("、")}。这些设置可能优先于 pip.ini。</div>
          </div>
        )}
        {scopes.map((scope) => {
          const k = pipKey(scope);
          const customWithoutFile = scope.kind === "custom" && !scope.path.trim();
          return (
            <div className="srcrow" key={k}>
              <span className="av py"><i className="ti ti-brand-python" /></span>
              <div className="mt">
                <div className="t">
                  {scope.name} {pipBadge(scope)} {scope.requires_admin && <span className="bd w">需 UAC</span>}
                </div>
                {scope.path
                  ? <div className="s dim" title={`当前配置：${scope.current_label}；实际生效：${scope.effective_label}`}>当前：{scope.current_label} · 生效：{scope.effective_label}</div>
                  : <div className="s dim" title="选择一个 pip.ini 文件后，可对该文件单独配置或清除镜像。">选择 pip.ini 后可单独配置。</div>}
                <div className={"s" + (scope.path ? " mono" : " dim")} title={scope.path || "未选择"}>{scope.path || "未选择"}</div>
              </div>
              {scope.kind === "custom" && (
                <button className="gh sm" disabled={!!busy} onClick={chooseCustomPipIni}>
                  <i className="ti ti-file-search" /> 选择文件
                </button>
              )}
              <Select value={pipSel[k] ?? scope.current ?? "official"} width={216}
                onChange={(v) => setPipSel((s) => ({ ...s, [k]: v }))}
                options={mirrorOptions(pipState.mirrors, fast)} />
              <button className="pr sm" disabled={!!busy || customWithoutFile} onClick={() => applyPip(scope)}>
                <i className={"ti " + (busy === "pip:" + k ? "ti-loader spin" : "ti-check")} /> 应用
              </button>
              <button className="gh sm" disabled={!!busy || customWithoutFile} onClick={() => clearPip(scope)}>
                <i className={"ti " + (busy === "pip-clear:" + k ? "ti-loader spin" : "ti-eraser")} /> 清除
              </button>
            </div>
          );
        })}
      </div>
    );
  }

  function renderCustomConfigRow(t: ToolState) {
    if (!CUSTOM_CONFIG_PATH_KEYS[t.id] || !t.installed) return null;
    const path = customConfigPaths[t.id] ?? "";
    const key = customConfigKey(t.id, path);
    const fast = fastestOf(t.mirrors);
    const noFile = !path.trim();
    const state = customConfigState[key];
    return (
      <div className="srcrow" key={`${t.id}-custom-config`}>
        <span className={"av " + (AV[t.id] ?? "st")}><i className={"ti " + (ICON[t.id] ?? "ti-package")} /></span>
        <div className="mt">
          <div className="t">{customConfigName(t.id)} <span className="bd n">自选</span> {customSourceBadge(path, state)}</div>
          <div className="s dim" title={customConfigHint(t.id)}>{customConfigHint(t.id)}</div>
          <div className={"s" + (noFile ? " dim" : " mono")} title={path || "未选择"}>{path || "未选择"}</div>
        </div>
        <button className="gh sm" disabled={!!busy} onClick={() => chooseCustomConfig(t.id)}>
          <i className="ti ti-file-search" /> 选择文件
        </button>
        {renderProxyToggle(t.id, customRowProxyKey(t.id, path), noFile)}
        <Select value={sel[key] ?? sel[t.id] ?? "official"} width={216}
          onChange={(v) => setSel((s) => ({ ...s, [key]: v }))}
          options={mirrorOptions(t.mirrors, fast)} />
        <button className="pr sm" disabled={!!busy || noFile} onClick={() => applyCustomConfig(t)}>
          <i className={"ti " + (busy === key ? "ti-loader spin" : "ti-check")} /> 应用
        </button>
        <button className="gh sm" disabled={!!busy || noFile} onClick={() => clearCustomConfig(t)}>
          <i className={"ti " + (busy === "clear:" + key ? "ti-loader spin" : "ti-eraser")} /> 清除
        </button>
      </div>
    );
  }

  function renderProxyToggle(toolId: string, stateKey?: string, disabled = false) {
    if (toolId !== "maven" && toolId !== "gradle") return null;
    const k = stateKey ?? rowProxyKey(toolId);
    return (
      <label className="ck" title={`启用后写入 JVM 工具代理：${proxyAddr.host}:${proxyAddr.port}，可在「设置」中修改代理地址。`}>
        <input type="checkbox" disabled={disabled} checked={!!sourceProxy[k]} onChange={(e) => setSourceProxy((s) => ({ ...s, [k]: e.target.checked }))} />
        代理
      </label>
    );
  }

  return (
    <>
      {anyInstalled && (
        <div className="srctoolbar">
          <div className="mt">
            <div className="s dim" title={hasJvmProxyTools ? "测速会根据连接延迟预选更快的镜像；Maven / Gradle 的代理开关会写入对应配置文件，代理地址在「设置」中维护；所有配置点击「应用」后生效。" : "测速会根据连接延迟预选更快的镜像；配置在点击「应用」后写入。"}>
              {hasJvmProxyTools ? "测速后预选镜像；代理开关随「应用」写入配置。" : "测速后预选更快镜像；点击「应用」后生效。"}
            </div>
          </div>
          <button className="gh sm" disabled={testing || !!busy} onClick={speedtest}>
            <i className={"ti " + (testing ? "ti-loader spin" : "ti-bolt")} /> {testing ? "测速中…" : "测速"}
          </button>
        </div>
      )}
      {tools.map((t) => {
        if (t.id === "pip") return renderPipRows(t);
        const fast = fastestOf(t.mirrors);
        return (
          <div key={t.id} style={{ display: "contents" }}>
            <div className="srcrow">
              <span className={"av " + (AV[t.id] ?? "st")}><i className={"ti " + (ICON[t.id] ?? "ti-package")} /></span>
              <div className="mt">
                <div className="t">{t.name} {badge(t)}</div>
                <div className={"s" + (t.installed ? " mono" : " dim")} title={t.installed ? t.config : missingText(t)}>{t.installed ? t.config : missingText(t)}</div>
              </div>
              {t.installed && (
                <>
                  {t.id === "go" && (
                    <Select value={sourceScopes.go ?? "user"} width={112}
                      onChange={(v) => setSourceScopes((s) => ({ ...s, go: v as "user" | "system" }))}
                      options={[
                        { value: "user", label: "当前用户" },
                        { value: "system", label: "系统级" },
                      ]} />
                  )}
                  {renderProxyToggle(t.id)}
                  <Select value={sel[t.id] ?? ""} width={216}
                    onChange={(v) => setSel((s) => ({ ...s, [t.id]: v }))}
                    options={mirrorOptions(t.mirrors, fast)} />
                  <button className="pr sm" disabled={busy === t.id} onClick={() => apply(t.id)}>
                    <i className={"ti " + (busy === t.id ? "ti-loader spin" : "ti-check")} /> {busy === t.id ? "应用中…" : "应用"}
                  </button>
                  {(t.id === "maven" || t.id === "gradle") && (
                    <button className="gh sm" disabled={!!busy} onClick={() => clearToolConfig(t)}>
                      <i className={"ti " + (busy === "clear:" + t.id ? "ti-loader spin" : "ti-eraser")} /> 清除
                    </button>
                  )}
                </>
              )}
            </div>
            {renderCustomConfigRow(t)}
          </div>
        );
      })}
    </>
  );
}
