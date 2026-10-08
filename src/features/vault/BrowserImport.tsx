import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { Modal, useToast } from "../../ui";
import { vaultApi, vaultError, type BrowserStats } from "./api";
import { BrowserPasswordHelpButton } from "./BrowserPasswordHelp";
import { useTitleProgress } from "./titleProgress";

const STEPS: [string, string][] = [
  ["Chrome", "打开 chrome://password-manager/settings，点「导出密码」旁的「下载文件」"],
  ["Edge", "打开 edge://wallet/passwords，点右上角「…」→「导出密码」"],
  ["Firefox", "打开 about:logins，点右上角「…」→「导出密码」"],
];

/** Logins a browser exported as CSV, added as general credentials that stay out of Windows Credential Manager. */
export function BrowserImport({ onClose, onChanged }: { onClose: () => void; onChanged: () => void }) {
  const toast = useToast();
  const { tr } = useI18n();
  const [src, setSrc] = useState("");
  const [stats, setStats] = useState<BrowserStats | null>(null);
  const [done, setDone] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [titles, setTitles] = useState(false);
  const [applying, setApplying] = useState(false);
  const progress = useTitleProgress(applying && titles);

  async function choose() {
    try {
      const picked = await open({ title: tr("选择浏览器导出的密码文件"), multiple: false, directory: false, filters: [{ name: "CSV", extensions: ["csv"] }] });
      if (typeof picked !== "string") return;
      setBusy(true);
      setSrc(picked);
      setDone(null);
      setStats(await vaultApi.importBrowser(picked, false));
    } catch (error) { setStats(null); toast(vaultError(error) === "文件已损坏，或不是有效的保管库文件。" ? tr("这不是浏览器导出的密码文件：找不到 password 这一列。") : vaultError(error), "err"); }
    finally { setBusy(false); }
  }
  async function apply() {
    setBusy(true);
    setApplying(true);
    try {
      const result = await vaultApi.importBrowser(src, true, titles);
      setDone(result.added);
      onChanged();
    } catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); setApplying(false); }
  }
  const folder = src.replace(/[\\/][^\\/]+$/, "");

  return <Modal wide title="导入浏览器密码" icon="ti-world-download" onClose={busy ? undefined : onClose}
    footer={done !== null
      ? <button className="pr sm" onClick={onClose}>完成</button>
      : <>
        <button className="gh sm" disabled={busy} onClick={onClose}>取消</button>
        <button className="pr sm" disabled={busy || !stats || stats.added === 0} onClick={() => void apply()}>
          {applying && <i className="ti ti-loader spin" />}
          {applying && titles
            ? progress ? tr("正在读取网页标题 {done}/{total}").replace("{done}", String(progress[0])).replace("{total}", String(progress[1])) : tr("正在读取网页标题…")
            : stats ? tr("导入 {count} 条").replace("{count}", String(stats.added)) : tr("导入")}
        </button>
      </>}>
    {done === null ? <div className="vault-form">
      <div className="pw-help-line"><span>浏览器不允许别的程序直接读取它保存的密码，请先在浏览器里导出成 CSV 文件，再在这里选择它。</span><BrowserPasswordHelpButton /></div>
      <div className="vault-import-steps">{STEPS.map(([name, step]) => <div key={name}><b>{name}</b><span>{tr(step)}</span></div>)}</div>
      <div className="vault-bar" style={{ margin: 0 }}>
        <button className="gh sm" disabled={busy} onClick={() => void choose()}><i className={"ti " + (busy && !stats ? "ti-loader spin" : "ti-folder-open")} /> 选择 CSV 文件</button>
        <span className="vault-import-path" translate="no" title={src || undefined}>{src || "未选择文件"}</span>
      </div>
      {stats && <div className="callout" style={{ margin: 0 }}><i className="ti ti-info-circle" /><div>
        {tr("新增 {added} 条，已在保管库 {same} 条，没有密码的 {empty} 行跳过。").replace("{added}", String(stats.added)).replace("{same}", String(stats.same)).replace("{empty}", String(stats.empty))}
      </div></div>}
      <label className="vault-check">
        <input type="checkbox" checked={titles} disabled={busy} onChange={(event) => setTitles(event.target.checked)} />
        <span>读取各网站的网页标题作为备注<span className="mut vault-check-more">会逐个打开这些网址（只读网页，不带账号信息），多的话要一两分钟；打不开的留空。以后也能在列表里勾选后补。</span></span>
      </label>
      <div className="vault-sub" style={{ margin: 0 }}>每条登录存成一条通用凭据（网址、账号、密码），打上「浏览器」标签；默认不放进 Windows 凭据管理器，需要的再在列表里勾选后放进去。</div>
    </div> : <div className="vault-form">
      <div className="callout" style={{ margin: 0 }}><i className="ti ti-circle-check" /><div>{tr("已导入 {count} 条。").replace("{count}", String(done))}</div></div>
      <div className="vault-warn">导出的 CSV 文件里密码是明文，用完请删掉它，并清空回收站。</div>
      <div><button className="gh sm" onClick={() => void invoke("space_open_directory", { path: folder }).catch((error) => toast(String(error), "err"))}><i className="ti ti-folder-open" /> 打开文件所在位置</button></div>
    </div>}
  </Modal>;
}
