import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../i18n";
import { useToast } from "../../ui";
import { vaultApi, vaultError, type DiscoverStatus, type Finding, type Kind } from "./api";
import { ENV_SCOPE_LABELS, RISK_LABELS, SOURCE_LABELS, STATUS_LABELS } from "./labels";

const SOURCE_ORDER: Finding["source"][] = ["ssh", "config", "env", "credential", "dotenv"];
const POLL_MS = 800;

export function groupFindings(findings: Finding[]): [string, Finding[]][] {
  return SOURCE_ORDER.map((source) => [source, findings.filter((finding) => finding.source === source)] as [string, Finding[]])
    .filter(([, items]) => items.length > 0);
}

export function locationText(finding: Finding): string {
  return finding.source === "env" ? ENV_SCOPE_LABELS[finding.location] ?? finding.location : finding.location;
}

type Choice = { platform: string; kind: Kind };

export function DiscoverPanel({ onImported }: { onImported: () => void }) {
  const toast = useToast();
  const { tr } = useI18n();
  const [scope, setScope] = useState({ ssh: true, configs: true, env: true, credentials: true });
  const [dirs, setDirs] = useState<string[]>([]);
  const [dirsReady, setDirsReady] = useState(false);
  const [dirsBusy, setDirsBusy] = useState(false);
  const [scanned, setScanned] = useState(false);
  const [status, setStatus] = useState<DiscoverStatus | null>(null);
  const [chosen, setChosen] = useState<Record<number, Choice>>({});
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(() => vaultApi.discoverStatus().then(setStatus).catch(() => undefined), []);

  useEffect(() => {
    let alive = true;
    vaultApi.settings()
      .then((settings) => { if (alive) { setDirs(settings.vault_scan_dirs); setDirsReady(true); } })
      .catch((error) => { if (alive) toast(vaultError(error), "err"); });
    void refresh();
    return () => { alive = false; };
  }, [refresh, toast]);

  // Findings hold plaintext values: leaving the tab drops them from backend memory.
  useEffect(() => () => { void vaultApi.discoverClear().catch(() => undefined); }, []);

  useEffect(() => {
    if (!status?.running) return;
    const timer = window.setInterval(() => void refresh(), POLL_MS);
    return () => window.clearInterval(timer);
  }, [status?.running, refresh]);

  // settings_set_vault replaces both fields wholesale: re-read right before saving so the auto-lock minutes are never stale.
  async function saveDirs(change: (current: string[]) => string[]) {
    setDirsBusy(true);
    try {
      const fresh = await vaultApi.settings();
      const next = change(fresh.vault_scan_dirs);
      if (next.length === fresh.vault_scan_dirs.length && next.every((dir, index) => dir === fresh.vault_scan_dirs[index])) { setDirs(next); return; }
      const saved = await vaultApi.setSettings(fresh.vault_auto_lock_minutes, next);
      setDirs(saved.vault_scan_dirs);
    } catch (error) { toast(vaultError(error), "err"); }
    finally { setDirsBusy(false); }
  }
  async function addDir() {
    let picked: string | string[] | null;
    setDirsBusy(true);
    try { picked = await open({ title: tr("添加项目文件夹"), directory: true, multiple: false }); }
    catch (error) { toast(vaultError(error), "err"); setDirsBusy(false); return; }
    setDirsBusy(false);
    const dir = typeof picked === "string" ? picked.trim() : "";
    if (dir) await saveDirs((current) => (current.includes(dir) ? current : [...current, dir]));
  }
  async function start() {
    setChosen({});
    try { await vaultApi.discoverStart({ ...scope, projectDirs: dirs }); setScanned(true); await refresh(); }
    catch (error) { toast(vaultError(error), "err"); }
  }
  async function cancel() {
    try { await vaultApi.discoverCancel(); await refresh(); }
    catch (error) { toast(vaultError(error), "err"); }
  }
  function toggle(finding: Finding) {
    setChosen((current) => {
      const next = { ...current };
      if (next[finding.id]) delete next[finding.id];
      else next[finding.id] = { platform: finding.platform, kind: finding.kind };
      return next;
    });
  }
  function edit(id: number, patch: Partial<Choice>) {
    setChosen((current) => (current[id] ? { ...current, [id]: { ...current[id], ...patch } } : current));
  }
  function chooseAllNew() {
    const next: Record<number, Choice> = {};
    status?.findings.filter((finding) => finding.status === "new").forEach((finding) => { next[finding.id] = { platform: finding.platform, kind: finding.kind }; });
    setChosen(next);
  }
  async function importChosen() {
    setBusy(true);
    try {
      // An environment variable's location is only its scope's id; the note gets the words the list shows.
      const env = new Map((status?.findings ?? []).filter((finding) => finding.source === "env").map((finding) => [finding.id, tr(locationText(finding))]));
      const items = Object.entries(chosen).map(([id, choice]) => ({
        id: Number(id), ...choice, ...(env.has(Number(id)) ? { origin: env.get(Number(id)) } : {}),
      }));
      const count = await vaultApi.discoverImport(items, tr("来源："));
      toast(`已导入 ${count} 项。原文件未做任何改动。`, "ok");
      setChosen({});
      await refresh();
      onImported();
    } catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }
  async function ignoreChosen() {
    setBusy(true);
    try { await vaultApi.discoverIgnore(Object.keys(chosen).map(Number)); setChosen({}); await refresh(); }
    catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }

  const running = status?.running ?? false;
  const findings = status?.findings ?? [];
  const selectedCount = Object.keys(chosen).length;
  const foldersLocked = !dirsReady || dirsBusy;
  // The backend reports an idle, empty status before any scan: only show a result once a scan has really happened.
  const hasResult = Boolean(status) && (scanned || status!.files > 0 || status!.cancelled || status!.truncated || findings.length > 0);

  return (
    <>
      <div className="pxcard">
        <div className="vault-sub">扫描本机常见位置中以明文保存的密钥，选择后加密保存到保管库。扫描只读，不会修改或删除原文件。</div>
        <div className="vault-form">
          <label style={{ flexDirection: "row", gap: 8 }}><input type="checkbox" checked={scope.ssh} onChange={(e) => setScope({ ...scope, ssh: e.target.checked })} /> SSH 密钥（~/.ssh）</label>
          <label style={{ flexDirection: "row", gap: 8 }}><input type="checkbox" checked={scope.configs} onChange={(e) => setScope({ ...scope, configs: e.target.checked })} /> 云与包管理配置（.aws、.npmrc、.git-credentials、.pypirc、.cargo）</label>
          <label style={{ flexDirection: "row", gap: 8 }}><input type="checkbox" checked={scope.env} onChange={(e) => setScope({ ...scope, env: e.target.checked })} /> 环境变量（用户与系统）</label>
          <label style={{ flexDirection: "row", gap: 8 }}><input type="checkbox" checked={scope.credentials} onChange={(e) => setScope({ ...scope, credentials: e.target.checked })} /> Windows 凭据管理器里 Git 保存的令牌（Git 页添加的 GitHub、Gitee 等账号）</label>
          <div>
            <div className="vault-sub" style={{ marginBottom: 6 }}>项目 .env（最多 4 层，跳过 node_modules、.git、target 等目录）</div>
            {dirs.map((dir) => (
              <div className="vault-bar" key={dir} style={{ marginBottom: 4 }}>
                <code className="grow" translate="no">{dir}</code>
                <button className="gh sm" title="移除" disabled={foldersLocked} onClick={() => void saveDirs((current) => current.filter((item) => item !== dir))}><i className="ti ti-x" /></button>
              </div>
            ))}
            <button className="gh sm" disabled={foldersLocked} onClick={() => void addDir()}><i className="ti ti-folder-plus" /> 添加文件夹</button>
          </div>
        </div>
        <div className="vault-actions">
          {running
            ? <button className="gh sm" onClick={() => void cancel()}>取消扫描</button>
            : <button className="pr sm" disabled={!dirsReady || (!scope.ssh && !scope.configs && !scope.env && !scope.credentials && dirs.length === 0)} onClick={() => void start()}><i className="ti ti-radar" /> 开始扫描</button>}
        </div>
      </div>

      {running && <div className="callout vault-note"><i className="ti ti-loader spin" /><div>正在扫描… 已检查 {status?.files ?? 0} 个文件</div></div>}
      {status?.truncated && <div className="callout vault-note"><i className="ti ti-alert-triangle" /><div>已达到单次 5 万个文件上限，结果可能不完整。</div></div>}
      {status?.cancelled && <div className="callout vault-note"><i className="ti ti-info-circle" /><div>扫描已取消，以下为已找到的结果。</div></div>}

      {hasResult && !running && (
        <div className="pxcard">
          {findings.length === 0 ? <div className="vault-empty">未发现明文密钥。</div> : (
            <>
              <div className="vault-bar">
                <button className="gh sm" disabled={busy} onClick={chooseAllNew}>选择全部新发现</button>
                <span className="grow" />
                <button className="gh sm" disabled={busy || selectedCount === 0} onClick={() => void ignoreChosen()}>忽略所选</button>
                <button className="pr sm" disabled={busy || selectedCount === 0} onClick={() => void importChosen()}>{tr("导入所选")} ({selectedCount})</button>
              </div>
              {groupFindings(findings).map(([source, items]) => (
                <div key={source}>
                  <div className="pxsec">{SOURCE_LABELS[source]}</div>
                  {items.map((finding) => {
                    const choice = chosen[finding.id];
                    return (
                      <div className="vault-field" key={finding.id} style={{ gridTemplateColumns: "24px minmax(0,1fr) auto" }}>
                        <input type="checkbox" aria-label={finding.name} disabled={busy || finding.status !== "new"} checked={Boolean(choice)} onChange={() => toggle(finding)} />
                        <div>
                          <div translate="no">{finding.name} <span className="mut">{finding.preview}</span></div>
                          <div className="mut" style={{ fontSize: 11.5 }} translate="no">{locationText(finding)}</div>
                          {finding.risks.length > 0 && <div className="vault-tags">{finding.risks.map((risk) => <span key={risk} className="vault-badge expired">{RISK_LABELS[risk] ?? risk}</span>)}</div>}
                          {choice && (
                            <div className="vault-bar" style={{ marginTop: 6, marginBottom: 0 }}>
                              <input className="ip" placeholder="平台" value={choice.platform} disabled={busy} onChange={(e) => edit(finding.id, { platform: e.target.value })} />
                            </div>
                          )}
                        </div>
                        <span className={"vault-badge" + (finding.status === "new" ? " soon" : "")}>{STATUS_LABELS[finding.status]}</span>
                      </div>
                    );
                  })}
                </div>
              ))}
            </>
          )}
        </div>
      )}
    </>
  );
}
