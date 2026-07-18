import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "./invoke";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Modal, ConfirmModal, useToast } from "./ui";
import { Select } from "./Select";

type SourceLayer = "all" | "builtin" | "local";
type CatalogRow = {
  row_id: string;
  tool_id: string;
  tool_name: string;
  category: string;
  category_label: string;
  source: "builtin" | "local";
  source_label: string;
  mirror_id: string;
  name: string;
  url: string;
  host: string;
  description: string;
  current: boolean;
  mutable: boolean;
  has_auth: boolean;
  duplicate: boolean;
};
type CatalogStatus = {
  server_url: string;
  server_version: string | null;
  builtin_count: number;
  local_count: number;
  binary_count: number;
  rows: CatalogRow[];
};
type MirrorsUpdateCheck = {
  url: string;
  local_version: string | null;
  remote_version: string;
  has_update: boolean;
  tools: number;
};
type Custom = { id: string; tool: string; name: string; url: string; username: string; has_password: boolean };
type Draft = { id: string; tool: string; name: string; url: string; username: string; pw: string; pwTouched: boolean };
type HostPing = { host: string; ms: number | null };
type ToolFilterOption = { value: string; label: string; count: number };

const AUTH_TOOLS = new Set(["pip", "npm", "yarn", "go", "maven", "gradle", "cargo"]);
const CATEGORY_ORDER = ["all", "runtime", "package", "build", "binary", "local"] as const;
const CATEGORY_LABEL: Record<string, string> = {
  all: "全部",
  runtime: "运行时下载",
  package: "包仓库",
  build: "构建工具",
  binary: "大文件下载",
  local: "本地自定义",
};

function emptyDraft(tool = "npm"): Draft {
  return { id: "", tool, name: "", url: "", username: "", pw: "", pwTouched: false };
}

function layerBadge(row: CatalogRow) {
  if (row.source === "local") return <span className="bd g">本地</span>;
  return <span className="bd n">内置</span>;
}

function msLabel(ms: number | null | undefined, active: boolean) {
  if (active && ms === undefined) return <span className="bd w"><i className="ti ti-loader spin" /> 测速中</span>;
  if (ms === undefined) return <span className="bd n">未测速</span>;
  if (ms === null) return <span className="bd off">超时</span>;
  if (ms <= 100) return <span className="bd g">{ms} ms</span>;
  if (ms <= 600) return <span className="bd w">{ms} ms</span>;
  return <span className="bd r">{ms} ms</span>;
}

function matchesCategory(row: CatalogRow, category: string) {
  if (category === "all") return true;
  if (category === "local") return row.source === "local";
  return row.category === category;
}

export function SourceManagerModal({ onClose, onChanged }: { onClose: () => void; onChanged?: () => void }) {
  const toast = useToast();
  const checkedRemote = useRef(false);
  const [catalog, setCatalog] = useState<CatalogStatus | null>(null);
  const [customs, setCustoms] = useState<Custom[]>([]);
  const [category, setCategory] = useState("all");
  const [toolFilter, setToolFilter] = useState("all");
  const [layer, setLayer] = useState<SourceLayer>("all");
  const [query, setQuery] = useState("");
  const [serverUrl, setServerUrl] = useState("");
  const [busy, setBusy] = useState("");
  const [testing, setTesting] = useState(false);
  const [pings, setPings] = useState<Record<string, number | null | undefined>>({});
  const [draft, setDraft] = useState<Draft | null>(null);
  const [deleteId, setDeleteId] = useState<string | null>(null);
  const [remoteUpdate, setRemoteUpdate] = useState<MirrorsUpdateCheck | null>(null);
  const [checkingRemote, setCheckingRemote] = useState(false);

  const checkRemoteUpdate = useCallback(async (url: string, manual = false) => {
    setCheckingRemote(true);
    try {
      const res = await invoke<MirrorsUpdateCheck>("mirrors_check_update", { url: url.trim() || null });
      if (res.has_update) {
        setRemoteUpdate(res);
      } else if (manual) {
        toast(`公共源清单已是最新（v${res.remote_version}）`, "ok");
      }
    } catch (e) {
      if (manual) toast("检查公共源清单失败：" + e, "err");
    } finally {
      setCheckingRemote(false);
    }
  }, [toast]);

  const load = useCallback(async () => {
    const next = await invoke<CatalogStatus>("source_catalog_status");
    setCatalog(next);
    setServerUrl((cur) => cur || next.server_url);
    try {
      setCustoms(await invoke<Custom[]>("custom_list"));
    } catch (error) {
      toast("读取本地自定义源失败：" + error, "err");
    }
    if (!checkedRemote.current) {
      checkedRemote.current = true;
      checkRemoteUpdate(next.server_url, false);
    }
  }, [checkRemoteUpdate, toast]);

  useEffect(() => { load().catch((e) => toast("读取源目录失败：" + e, "err")); }, [load, toast]);

  const toolOptions = useMemo(() => {
    const map = new Map<string, string>();
    catalog?.rows.forEach((row) => {
      if (row.category !== "binary") map.set(row.tool_id, row.tool_name);
    });
    return [...map.entries()].map(([value, label]) => ({ value, label }));
  }, [catalog]);

  const toolFilterOptions = useMemo<ToolFilterOption[]>(() => {
    const scoped = (catalog?.rows ?? []).filter((row) => matchesCategory(row, category));
    const map = new Map<string, ToolFilterOption>();
    for (const row of scoped) {
      const prev = map.get(row.tool_id);
      if (prev) prev.count++;
      else map.set(row.tool_id, { value: row.tool_id, label: row.tool_name, count: 1 });
    }
    return [
      { value: "all", label: "全部", count: scoped.length },
      ...[...map.values()].sort((a, b) => a.label.localeCompare(b.label)),
    ];
  }, [catalog, category]);

  const toolSelectOptions = useMemo(() => toolFilterOptions.map((opt) => {
    const label = opt.value === "all" ? "全部场景" : opt.label;
    return { value: opt.value, label: `${label} ${opt.count}`, title: `${label}（${opt.count} 个）` };
  }), [toolFilterOptions]);

  useEffect(() => {
    if (toolFilter === "all") return;
    if (!toolFilterOptions.some((opt) => opt.value === toolFilter)) setToolFilter("all");
  }, [toolFilter, toolFilterOptions]);

  const rows = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (catalog?.rows ?? []).filter((row) => {
      if (!matchesCategory(row, category)) return false;
      if (toolFilter !== "all" && row.tool_id !== toolFilter) return false;
      if (layer !== "all" && row.source !== layer) return false;
      if (!q) return true;
      return [row.name, row.tool_name, row.url, row.host, row.description].some((v) => v.toLowerCase().includes(q));
    }).sort((a, b) => {
      const ma = pings[a.host];
      const mb = pings[b.host];
      const ka = ma === undefined ? 1e12 : ma ?? 1e11;
      const kb = mb === undefined ? 1e12 : mb ?? 1e11;
      if (ka !== kb) return ka - kb;
      return a.tool_name.localeCompare(b.tool_name) || a.name.localeCompare(b.name);
    });
  }, [catalog, category, toolFilter, layer, query, pings]);

  const counts = useMemo(() => {
    const m: Record<string, number> = {};
    for (const key of CATEGORY_ORDER) m[key] = 0;
    for (const row of catalog?.rows ?? []) {
      m.all++;
      m[row.category] = (m[row.category] ?? 0) + 1;
      if (row.source === "local") m.local++;
    }
    return m;
  }, [catalog]);

  function openNew() {
    const selectedTool = toolFilter !== "all" && toolOptions.some((opt) => opt.value === toolFilter)
      ? toolFilter
      : toolOptions[0]?.value;
    setDraft(emptyDraft(selectedTool ?? "npm"));
  }

  function openEdit(c: Custom) {
    setDraft({ id: c.id, tool: c.tool, name: c.name, url: c.url, username: c.username, pw: "", pwTouched: false });
  }

  async function saveDraft() {
    if (!draft) return;
    if (!draft.name.trim()) { toast("请输入源名称", "info"); return; }
    if (!/^(https?:\/\/|sparse\+)/.test(draft.url.trim())) { toast("地址需以 http(s):// 或 sparse+ 开头", "info"); return; }
    setBusy("draft");
    try {
      const args: Record<string, unknown> = {
        id: draft.id || null,
        tool: draft.tool,
        name: draft.name,
        url: draft.url,
        username: draft.username,
      };
      if (!draft.id || draft.pwTouched) args.password = draft.pw;
      await invoke("custom_save", args);
      setDraft(null);
      await load();
      onChanged?.();
      toast(draft.id ? "本地源已更新" : "本地源已创建", "ok");
    } catch (e) {
      toast("保存本地源失败：" + e, "err");
    } finally {
      setBusy("");
    }
  }

  async function deleteCustom(id: string) {
    setBusy("delete");
    try {
      await invoke("custom_delete", { id });
      setDeleteId(null);
      await load();
      onChanged?.();
      toast("本地源已删除", "ok");
    } catch (e) {
      toast("删除本地源失败：" + e, "err");
    } finally {
      setBusy("");
    }
  }

  async function updateServer(urlOverride?: string) {
    const target = (urlOverride ?? serverUrl).trim();
    if (!target) { toast("请输入服务器清单地址", "info"); return; }
    setBusy("server");
    try {
      const s = await invoke<{ local_version: string | null; tools: number }>("mirrors_update", { url: target });
      setRemoteUpdate(null);
      await load();
      onChanged?.();
      toast(`服务器清单已更新到 v${s.local_version}（${s.tools} 个分组）`, "ok");
    } catch (e) {
      toast("拉取服务器清单失败：" + e, "err");
    } finally {
      setBusy("");
    }
  }

  async function exportSources() {
    try {
      const path = await save({ defaultPath: "stacker-sources.json", filters: [{ name: "Stacker 源配置", extensions: ["json"] }] });
      if (!path) return;
      await invoke("source_catalog_export", { path, includeServer: false });
      toast("源配置已导出，已包含内置源快照和本地源", "ok");
    } catch (e) {
      toast("导出源配置失败：" + e, "err");
    }
  }

  async function importSources() {
    try {
      const path = await open({ multiple: false, directory: false, filters: [{ name: "Stacker 源配置", extensions: ["json"] }] });
      if (!path || typeof path !== "string") return;
      const r = await invoke<{ local_added: number; local_skipped: number; server_imported: boolean; builtin_tools: number; builtin_mirrors: number }>("source_catalog_import", { path, importServer: true });
      await load();
      onChanged?.();
      const builtin = r.server_imported ? `，已恢复内置源 ${r.builtin_tools} 组 / ${r.builtin_mirrors} 条` : "";
      toast(`已导入本地源 ${r.local_added} 个，跳过 ${r.local_skipped} 个${builtin}`, "ok");
    } catch (e) {
      toast("导入源配置失败：" + e, "err");
    }
  }

  async function speedtest() {
    const hosts = [...new Set(rows.map((row) => row.host).filter(Boolean))];
    if (hosts.length === 0) { toast("当前筛选结果没有可测速主机", "info"); return; }
    const next: Record<string, number | null | undefined> = {};
    hosts.forEach((host) => { next[host] = undefined; });
    setPings((prev) => ({ ...prev, ...next }));
    setTesting(true);
    try {
      const res = await invoke<HostPing[]>("speedtest_hosts", { hosts });
      const done: Record<string, number | null> = {};
      res.forEach((r) => { done[r.host] = r.ms; });
      hosts.forEach((host) => { if (!(host in done)) done[host] = null; });
      setPings((prev) => ({ ...prev, ...done }));
      toast("测速完成，列表已按延迟排序", "ok");
    } catch (e) {
      toast("测速失败：" + e, "err");
    } finally {
      setTesting(false);
    }
  }

  const selectedCustom = deleteId ? customs.find((c) => c.id === deleteId) : null;

  return (
    <>
      <div className="source-manager-modal">
        <Modal wide title="源管理" icon="ti-database-cog"
          sub={<div className="sm-summary">
            <span>内置 {catalog?.builtin_count ?? 0}</span>
            <span>服务器清单{catalog?.server_version ? ` v${catalog.server_version}` : "未更新"}</span>
            <span>本地 {catalog?.local_count ?? 0}</span>
            <span>大文件 {catalog?.binary_count ?? 0}</span>
          </div>}
          onClose={onClose}
          footer={<>
            <button className="gh sm" onClick={importSources}><i className="ti ti-download" /> 导入</button>
            <button className="gh sm" onClick={exportSources}><i className="ti ti-upload" /> 导出</button>
            <button className="pr sm" onClick={onClose}>完成</button>
          </>}>
          <div className="sourcemgr">
          <div className="smnav">
            {CATEGORY_ORDER.map((key) => (
              <button key={key} className={category === key ? "on" : ""} onClick={() => { setCategory(key); setToolFilter("all"); }}>
                <span>{CATEGORY_LABEL[key]}</span>
                <b>{counts[key] ?? 0}</b>
              </button>
            ))}
          </div>
          <div className="smpanel">
            <div className="smserver">
              <div className="mt">
                <div className="t">服务器清单</div>
                <div className="s dim" title="从服务器拉取最新公共源清单，并全量替换内置源；本地自定义源不会被覆盖。">拉取后全量更新内置源；本地自定义源不会被覆盖。</div>
              </div>
              <input className="ip full" value={serverUrl} title={serverUrl} placeholder="https://raw.githubusercontent.com/user/repo/main/mirrors.json"
                onChange={(e) => setServerUrl(e.target.value)} />
              <div className="smserver-actions">
                <button className="gh sm" disabled={checkingRemote || busy === "server"} onClick={() => checkRemoteUpdate(serverUrl, true)}>
                  <i className={"ti " + (checkingRemote ? "ti-loader spin" : "ti-refresh")} /> {checkingRemote ? "检查中…" : "检查更新"}
                </button>
                <button className="pr sm" disabled={busy === "server"} onClick={() => updateServer()}>
                  <i className={"ti " + (busy === "server" ? "ti-loader spin" : "ti-cloud-download")} /> 拉取最新
                </button>
              </div>
            </div>

            <div className="smtools">
              <input className="ip full" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="搜索名称、场景、地址或主机" />
              <Select value={toolFilter} width={142} onChange={setToolFilter} options={toolSelectOptions} />
              <Select value={layer} width={118} onChange={(v) => setLayer(v as SourceLayer)}
                options={[
                  { value: "all", label: "全部来源" },
                  { value: "builtin", label: "内置" },
                  { value: "local", label: "本地" },
                ]} />
              <button className="gh sm" disabled={testing} onClick={speedtest}>
                <i className={"ti " + (testing ? "ti-loader spin" : "ti-bolt")} /> {testing ? "测速中…" : "测速"}
              </button>
              <button className="pr sm" onClick={() => openNew()}><i className="ti ti-plus" /> 新建本地源</button>
            </div>

            <div className="smlist">
              {!catalog ? <div className="empty"><div className="ei"><i className="ti ti-loader spin" /></div><div className="eh">正在读取源目录</div></div>
                : rows.length === 0 ? <div className="empty"><div className="ei"><i className="ti ti-search-off" /></div><div className="eh">没有匹配的源</div><div className="ed">调整分类、来源或搜索条件后重试。</div></div>
                  : rows.map((row) => {
                    const local = customs.find((c) => c.id === row.mirror_id);
                    return (
                      <div className={"smrow" + (row.current ? " cur" : "")} key={row.row_id}>
                        <div className="smrank">{msLabel(pings[row.host], testing && !!row.host)}</div>
                        <div className="mt">
                          <div className="t">{row.name} {layerBadge(row)} {row.current && <span className="bd g">当前</span>} {row.has_auth && <span className="bd n"><i className="ti ti-lock" /> 鉴权</span>} {row.duplicate && <span className="bd w">重复</span>}</div>
                          <div className="s dim" title={`${row.tool_name} · ${row.category_label}`}>{row.tool_name} · {row.category_label}</div>
                          <div className="s mono" title={row.description || row.url}>{row.url || row.description || "由工具默认地址提供"}</div>
                        </div>
                        <div className="smhost" title={row.host}>{row.host || "默认"}</div>
                        {row.source === "local" && local
                          ? <>
                              <button className="gh sm" onClick={() => openEdit(local)}><i className="ti ti-pencil" /> 编辑</button>
                              <button className="gh sm" onClick={() => setDeleteId(local.id)}><i className="ti ti-trash" /></button>
                            </>
                          : null}
                      </div>
                    );
                  })}
            </div>
          </div>
          </div>
        </Modal>
      </div>

      {draft && (
        <Modal title={draft.id ? "编辑本地源" : "新建本地源"} icon="ti-key"
          sub="本地源只保存在当前电脑，不会被服务器清单覆盖。"
          onClose={() => busy !== "draft" && setDraft(null)}
          footer={<>
            <button className="gh sm" disabled={busy === "draft"} onClick={() => setDraft(null)}>取消</button>
            <button className="pr sm" disabled={busy === "draft"} onClick={saveDraft}>
              <i className={"ti " + (busy === "draft" ? "ti-loader spin" : "ti-check")} /> 保存
            </button>
          </>}>
          <div className="field">
            <label>所属场景</label>
            <Select value={draft.tool} disabled={!!draft.id} width="100%" onChange={(v) => setDraft({ ...draft, tool: v })}
              options={toolOptions.length ? toolOptions : [{ value: "npm", label: "npm / pnpm" }]} />
            {!AUTH_TOOLS.has(draft.tool) &&
              <div className="hint"><i className="ti ti-alert-triangle" style={{ color: "var(--amber)" }} /> 此场景暂只维护地址，用户名 / Token 不会写入工具配置。</div>}
          </div>
          <div className="field"><label>名称</label>
            <input className="ip full" autoFocus value={draft.name} placeholder="如：公司 Nexus"
              onChange={(e) => setDraft({ ...draft, name: e.target.value })} /></div>
          <div className="field"><label>地址</label>
            <input className="ip full" value={draft.url} placeholder="https://nexus.example.com/repository/npm/"
              onChange={(e) => setDraft({ ...draft, url: e.target.value })} /></div>
          <div className="field"><label>用户名 <span className="hint">选填</span></label>
            <input className="ip full" value={draft.username} placeholder="留空时，Token 会按对应工具规则写入"
              onChange={(e) => setDraft({ ...draft, username: e.target.value })} /></div>
          <div className="field"><label>密码 / Token <span className="hint">选填 · 本机加密保存</span></label>
            <input className="ip full" type="password" value={draft.pw}
              placeholder={draft.id ? "留空表示不修改原凭据" : "选填"}
              onChange={(e) => setDraft({ ...draft, pw: e.target.value, pwTouched: true })} />
          </div>
        </Modal>
      )}

      {selectedCustom && (
        <ConfirmModal title="删除本地源" icon="ti-trash" danger busy={busy === "delete"}
          message={<>确定删除「<b style={{ color: "var(--tx)" }}>{selectedCustom.name}</b>」？<br />这只删除源管理中的本地记录，不会回滚已写入的工具配置文件。</>}
          confirmLabel="删除" onConfirm={() => deleteCustom(selectedCustom.id)} onClose={() => setDeleteId(null)} />
      )}

      {remoteUpdate && (
        <ConfirmModal title="更新公共源清单" icon="ti-cloud-download" busy={busy === "server"}
          message={<>发现新版公共源清单：<b style={{ color: "var(--tx)" }}>v{remoteUpdate.remote_version}</b>{remoteUpdate.local_version ? <>（当前 v{remoteUpdate.local_version}）</> : <>（本机尚未同步）</>}。<br />更新后会全量替换内置源，本地自定义源不会被覆盖。</>}
          confirmLabel="更新" onConfirm={() => updateServer(remoteUpdate.url)} onClose={() => setRemoteUpdate(null)} />
      )}
    </>
  );
}
