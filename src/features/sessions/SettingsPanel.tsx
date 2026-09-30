import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../i18n";
import { BrowserExtension } from "./BrowserExtension";
import { getRoots, openSession, setExportDir } from "./api";
import { errorMessage, type RootsView } from "./types";


/** The 设置 tab: the browser extension, and where exports are written. The AI that writes
 *  summaries is chosen once, under Preferences → AI. */
export function SettingsPanel() {
  const { tr: t } = useI18n();
  const [view, setView] = useState<RootsView | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getRoots().then(setView).catch((e) => setError(errorMessage(e)));
  }, []);

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
