import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../i18n";
import { Modal } from "../../ui";
import { BrowserExtension } from "./BrowserExtension";
import { getRoots, getSummarySettings, openSession, runnerOptions, saveSummarySettings, setExportDir, setRoots } from "./api";
import { cleanSettings, RunnerFields } from "./RunnerFields";
import { errorMessage, type AgentOptions, type Roots, type RootsView, type SummarySettings } from "./types";

const ROWS: { key: keyof Roots; label: string; hint: string }[] = [
  { key: "codex", label: "Codex 数据目录", hint: "默认读取 CODEX_HOME，未设置时为用户目录下的 .codex" },
  { key: "claude", label: "Claude 数据目录", hint: "默认读取 CLAUDE_CONFIG_DIR，未设置时为用户目录下的 .claude" },
  { key: "claudeDesktopIndex", label: "Claude 桌面端会话索引", hint: "桌面端侧栏的会话列表，用于标题、归档和孤儿识别" },
];

/** The 设置 tab: summary defaults, the browser extension, and where exports are written. */
export function SettingsPanel({ onSaved }: { onSaved: () => void }) {
  const { tr: t } = useI18n();
  const [view, setView] = useState<RootsView | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [advanced, setAdvanced] = useState(false);

  const [summary, setSummary] = useState<SummarySettings | null>(null);
  const [savedSummary, setSavedSummary] = useState("");
  const [options, setOptions] = useState<AgentOptions[]>([]);

  useEffect(() => {
    Promise.all([getSummarySettings(), runnerOptions()])
      .then(([s, o]) => { setSummary(s); setSavedSummary(JSON.stringify(s)); setOptions(o); })
      .catch((e) => setError(errorMessage(e)));
  }, []);

  useEffect(() => {
    getRoots().then(setView).catch((e) => setError(errorMessage(e)));
  }, []);

  async function saveSummary() {
    if (!summary) return;
    setBusy(true); setError("");
    try { const clean = cleanSettings(summary); await saveSummarySettings(clean); setSummary(clean); setSavedSummary(JSON.stringify(clean)); onSaved(); }
    catch (e) { setError(errorMessage(e)); }
    finally { setBusy(false); }
  }

  async function chooseExportDir() {
    const path = await open({ directory: true, multiple: false, title: t("选择精简导出目录") });
    if (typeof path !== "string") return;
    setBusy(true); setError("");
    try { setView(await setExportDir(path)); }
    catch (e) { setError(errorMessage(e)); }
    finally { setBusy(false); }
  }

  async function resetExportDir() {
    setBusy(true); setError("");
    try { setView(await setExportDir("")); }
    catch (e) { setError(errorMessage(e)); }
    finally { setBusy(false); }
  }

  if (!view) return error ? <p role="alert" className="session-error">{t(error)}</p> : null;
  const custom = ROWS.some(({ key }) => !!view.overrides[key]);
  return <div className="session-sources">
    {summary && <div className="session-source summary-settings">
      <div className="session-source-head"><b>{t("摘要")}</b><small>{t("生成会话摘要和交接资料时使用的本机智能体、模型与推理强度。留空表示使用该命令行自己的默认值。")}</small></div>
      <RunnerFields value={summary} options={options} onChange={setSummary} />
      <div><button className="pr sm" disabled={busy || JSON.stringify(summary) === savedSummary} onClick={() => void saveSummary()}><i className="ti ti-device-floppy" />{t("保存摘要设置")}</button></div>
    </div>}
    <BrowserExtension />
    <div className="session-source">
      <div className="session-source-head"><b>{t("精简导出目录")}</b><small>{t("删除前导出的 Markdown 保存在这里")}</small></div>
      <div className="session-source-edit">
        <code title={view.exportDir}>{view.exportDir}</code>
        <button className="gh sm" disabled={busy} onClick={() => void chooseExportDir()}><i className="ti ti-folder" />{t("选择…")}</button>
        <button className="gh sm" disabled={busy} onClick={() => void resetExportDir()}>{t("恢复默认")}</button>
        <button className="gh sm" onClick={() => void openSession("", "exports").catch((e) => setError(errorMessage(e)))}><i className="ti ti-folder-open" />{t("打开")}</button>
      </div>
    </div>
    {/* Almost nobody keeps agent data somewhere Stacker cannot find; the few who do open
        this from one line instead of scrolling past three fields every visit. */}
    <p className="session-note fp-advanced">
      {t("智能体数据不在默认位置？")}
      <button className="linkish" onClick={() => setAdvanced(true)}>{t("自定义读取位置…")}</button>
      {custom && <span className="bd g">{t("已自定义")}</span>}
    </p>
    {advanced && <AdvancedRoots view={view} onClose={(next) => { setAdvanced(false); if (next) { setView(next); onSaved(); } }} />}
    {error && <p role="alert" className="session-error">{t(error)}</p>}
  </div>;
}

/** Where Stacker reads each agent's data, for the rare machine that keeps it elsewhere. */
function AdvancedRoots({ view, onClose }: { view: RootsView; onClose: (next: RootsView | null) => void }) {
  const { tr: t } = useI18n();
  const [draft, setDraft] = useState<Roots>(view.overrides);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const dirty = ROWS.some(({ key }) => draft[key] !== view.overrides[key]);

  async function choose(key: keyof Roots) {
    const path = await open({ directory: true, multiple: false });
    if (typeof path === "string") setDraft({ ...draft, [key]: path });
  }

  async function save() {
    setBusy(true); setError("");
    try { onClose(await setRoots(draft)); }
    catch (e) { setError(errorMessage(e)); setBusy(false); }
  }

  return <Modal wide title={t("自定义读取位置")} icon="ti-folder-cog" onClose={busy ? undefined : () => onClose(null)}
    footer={<>
      <button className="gh sm" disabled={busy} onClick={() => onClose(null)}>{t("取消")}</button>
      <button className="pr sm" disabled={busy || !dirty} onClick={() => void save()}><i className="ti ti-device-floppy" />{t("保存")}</button>
    </>}>
    <p className="session-note"><i className="ti ti-alert-triangle" /> {t("这里只改变 Stacker 从哪里读取数据，不会移动任何文件，智能体自己也不会跟着改。要把数据移到其他盘释放空间，请用「占用」标签里每个智能体的「迁移到其他盘」。")}</p>
    {ROWS.map(({ key, label, hint }) => <div className="session-source" key={key}>
      <div className="session-source-head"><b>{t(label)}</b><small>{t(hint)}</small></div>
      <code title={view.effective[key]}>{view.effective[key]}</code>
      <div className="session-source-edit">
        <input className="ip full" value={draft[key]} placeholder={t("使用默认位置")} aria-label={t(label)} onChange={(e) => setDraft({ ...draft, [key]: e.target.value })} />
        <button className="gh sm" onClick={() => void choose(key)}><i className="ti ti-folder" />{t("选择…")}</button>
        <button className="gh sm" disabled={!draft[key]} onClick={() => setDraft({ ...draft, [key]: "" })}>{t("恢复默认")}</button>
      </div>
    </div>)}
    {error && <p role="alert" className="session-error">{t(error)}</p>}
  </Modal>;
}
