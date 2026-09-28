import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../i18n";
import { BrowserExtension } from "./BrowserExtension";
import { getRoots, getSummarySettings, openSession, runnerOptions, saveSummarySettings, setExportDir } from "./api";
import { cleanSettings, RunnerFields } from "./RunnerFields";
import { errorMessage, type AgentOptions, type RootsView, type SummarySettings } from "./types";


/** The 设置 tab: summary defaults, the browser extension, and where exports are written. */
export function SettingsPanel({ onSaved }: { onSaved: () => void }) {
  const { tr: t } = useI18n();
  const [view, setView] = useState<RootsView | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

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
    {error && <p role="alert" className="session-error">{t(error)}</p>}
  </div>;
}
