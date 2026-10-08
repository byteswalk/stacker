import { useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { Modal, useToast } from "../../ui";
import { vaultApi, vaultError, type BrowserKind } from "./api";
import { BrowserPasswordHelpButton } from "./BrowserPasswordHelp";

const BROWSERS: { id: BrowserKind; name: string; step: string }[] = [
  { id: "chrome", name: "Chrome", step: "打开 chrome://password-manager/settings，点「导入密码」旁的「选择文件」" },
  { id: "edge", name: "Edge", step: "打开 edge://wallet/passwords，点右上角「…」→「导入密码」" },
  { id: "firefox", name: "Firefox", step: "打开 about:logins，点右上角「…」→「从文件导入」" },
];

function today(): string {
  const now = new Date();
  return `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, "0")}${String(now.getDate()).padStart(2, "0")}`;
}

/**
 * Logins written as the password file a browser imports. `ids` are the chosen entries; none
 * means every login in the vault.
 */
export function BrowserExport({ ids, onClose }: { ids: string[]; onClose: () => void }) {
  const toast = useToast();
  const { tr } = useI18n();
  const [browser, setBrowser] = useState<BrowserKind>("chrome");
  const [count, setCount] = useState<number | null>(null);
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<{ count: number; path: string } | null>(null);
  const chosen = BROWSERS.find((item) => item.id === browser)!;

  useEffect(() => {
    let alive = true;
    setCount(null);
    vaultApi.browserExportable(ids, browser).then((n) => { if (alive) setCount(n); }).catch(() => { if (alive) setCount(0); });
    return () => { alive = false; };
  }, [ids, browser]);

  async function submit() {
    setBusy(true);
    try {
      const dest = await save({ title: tr("导出浏览器密码"), defaultPath: `Stacker-passwords-${browser}-${today()}.csv`, filters: [{ name: "CSV", extensions: ["csv"] }] });
      if (!dest) return;
      const written = await vaultApi.exportBrowser(password, ids, browser, dest);
      setDone({ count: written, path: dest });
    } catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }

  const folder = done?.path.replace(/[\\/][^\\/]+$/, "") ?? "";
  return <Modal wide title="导出为浏览器密码" icon="ti-world-upload" onClose={busy ? undefined : onClose}
    footer={done
      ? <button className="pr sm" onClick={onClose}>完成</button>
      : <>
        <button className="gh sm" disabled={busy} onClick={onClose}>取消</button>
        <button className="pr sm" disabled={busy || !password || !count} onClick={() => void submit()}>
          {busy && <i className="ti ti-loader spin" />}{tr("选择位置并导出 {count} 条").replace("{count}", String(count ?? 0))}
        </button>
      </>}>
    {done ? <div className="vault-form">
      <div className="callout" style={{ margin: 0 }}><i className="ti ti-circle-check" /><div>
        {tr("已导出 {count} 条登录，可以导入 {browser}。").replace("{count}", String(done.count)).replace("{browser}", chosen.name)}
      </div></div>
      <div className="vault-import-steps"><div><b>{chosen.name}</b><span>{tr(chosen.step)}</span></div></div>
      <div className="vault-warn">这个文件里的密码是明文。导入浏览器后请删掉它，并清空回收站。</div>
      <div><button className="gh sm" onClick={() => void invoke("space_open_directory", { path: folder }).catch((error) => toast(String(error), "err"))}><i className="ti ti-folder-open" /> 打开文件所在位置</button></div>
    </div> : <div className="vault-form">
      <div className="pw-help-line">
        <span>{ids.length ? tr("导出选中的条目里有网址和密码的登录。") : tr("导出保管库里所有有网址和密码的登录。")}SSH 密钥和没有网址的条目不导出；同一地址同一账号存了几份的，只导出最新的一份。</span>
        <BrowserPasswordHelpButton />
      </div>
      <div className="seg" role="radiogroup" aria-label="导出给哪个浏览器">
        {BROWSERS.map((item) => <button key={item.id} role="radio" aria-checked={browser === item.id} className={browser === item.id ? "on" : ""}
          disabled={busy} onClick={() => setBrowser(item.id)}>{item.name}</button>)}
      </div>
      <div className="vault-import-steps"><div><b>{tr("导入")}</b><span>{tr(chosen.step)}</span></div></div>
      {browser === "firefox" && <div className="vault-sub" style={{ margin: 0 }}>Firefox 只记网站的协议、域名和端口，不记路径：同一网站同一账号的几条在它看来是同一条，文件里只放最近改过的那条，免得导入时报重复或冲突。手机 App 的登录它不收，不导出。</div>}
      <div className="callout" style={{ margin: 0 }}><i className="ti ti-info-circle" /><div>
        {count === null ? tr("正在计算…") : tr("将导出 {count} 条登录。").replace("{count}", String(count))}
      </div></div>
      <label>主密码<input className="ip full" type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} /></label>
      <div className="vault-warn">导出的文件里密码是明文，只给当前 Windows 用户读取；导入浏览器后请删掉它。</div>
    </div>}
  </Modal>;
}
