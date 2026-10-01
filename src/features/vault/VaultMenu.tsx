import { useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Modal, useToast } from "../../ui";
import { vaultApi, vaultError, type Credential, type MergeStats } from "./api";
import { AUTO_LOCK_CHOICES } from "./labels";
import { PasswordForm } from "./PasswordForm";
import { RecoveryKeyStep } from "./RecoveryKeyStep";
import { TrashDialog } from "./TrashDialog";

const BACKUP_FILTERS = [{ name: "Stacker 保管库", extensions: ["skv"] }];
type Dialog = "password" | "recovery" | "export" | "import" | "trash" | "autolock" | null;

function today(): string {
  const now = new Date();
  return `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, "0")}${String(now.getDate()).padStart(2, "0")}`;
}

export function VaultMenu({ onChanged }: { onChanged: () => void }) {
  const [openMenu, setOpenMenu] = useState(false);
  const [dialog, setDialog] = useState<Dialog>(null);
  const pick = (next: Dialog) => { setOpenMenu(false); setDialog(next); };
  const close = () => setDialog(null);
  const items: [Dialog, string, string][] = [
    ["password", "ti-key", "修改主密码"],
    ["recovery", "ti-lifebuoy", "重置恢复密钥"],
    ["export", "ti-file-export", "导出加密备份"],
    ["import", "ti-file-import", "导入备份"],
    ["trash", "ti-trash", "回收站"],
    ["autolock", "ti-clock-lock", "自动锁定"],
  ];
  return (
    <span style={{ position: "relative" }}>
      <button className="gh sm" title="更多" aria-expanded={openMenu} onClick={() => setOpenMenu(!openMenu)}><i className="ti ti-dots" /></button>
      {openMenu && (
        <div className="pxcard" role="menu" style={{ position: "absolute", right: 0, top: 32, zIndex: 20, padding: 6, minWidth: 168 }}>
          {items.map(([id, icon, label]) => (
            <button key={id} role="menuitem" className="gh sm" style={{ display: "flex", width: "100%", justifyContent: "flex-start" }} onClick={() => pick(id)}>
              <i className={"ti " + icon} /> {label}
            </button>
          ))}
        </div>
      )}
      {dialog === "password" && <ChangePasswordDialog onClose={close} />}
      {dialog === "recovery" && <ResetRecoveryDialog onClose={close} />}
      {dialog === "export" && <ExportDialog onClose={close} />}
      {dialog === "import" && <ImportDialog onClose={close} onChanged={onChanged} />}
      {dialog === "trash" && <TrashDialog onClose={close} onChanged={onChanged} />}
      {dialog === "autolock" && <AutoLockDialog onClose={close} />}
    </span>
  );
}

function ChangePasswordDialog({ onClose }: { onClose: () => void }) {
  const toast = useToast();
  const [current, setCurrent] = useState("");
  const [busy, setBusy] = useState(false);
  async function submit(next: string) {
    setBusy(true);
    try { await vaultApi.changePassword(current, next); toast("主密码已修改，恢复密钥保持有效。", "ok"); onClose(); }
    catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }
  return (
    <Modal title="修改主密码" icon="ti-key" onClose={busy ? undefined : onClose}>
      <div className="vault-form">
        <label>当前主密码<input className="ip full" type="password" autoComplete="current-password" value={current} onChange={(e) => setCurrent(e.target.value)} /></label>
      </div>
      <PasswordForm submitLabel="修改" busy={busy} onSubmit={(next) => void submit(next)} />
    </Modal>
  );
}

function ResetRecoveryDialog({ onClose }: { onClose: () => void }) {
  const toast = useToast();
  const [password, setPassword] = useState("");
  const [key, setKey] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  async function begin() {
    setBusy(true);
    try { setKey(await vaultApi.resetRecovery(password)); setPassword(""); }
    catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }
  return (
    <Modal title="重置恢复密钥" icon="ti-lifebuoy" onClose={busy || key ? undefined : onClose}>
      {key ? (
        <RecoveryKeyStep recoveryKey={key} onCancel={onClose}
          onConfirmed={() => { toast("已启用新的恢复密钥，原恢复密钥已失效。", "ok"); onClose(); }} />
      ) : (
        <div className="vault-form">
          <div className="vault-sub">将生成新的恢复密钥，原恢复密钥立即失效。</div>
          <label>主密码<input className="ip full" type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} /></label>
          <div className="vault-actions">
            <button className="gh sm" disabled={busy} onClick={onClose}>取消</button>
            <button className="pr sm" disabled={busy || !password} onClick={() => void begin()}>继续</button>
          </div>
        </div>
      )}
    </Modal>
  );
}

function ExportDialog({ onClose }: { onClose: () => void }) {
  const toast = useToast();
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  async function submit() {
    const dest = await save({ title: "导出加密备份", defaultPath: `Stacker-保管库-${today()}.skv`, filters: BACKUP_FILTERS });
    if (!dest) return;
    setBusy(true);
    try { await vaultApi.exportBackup(password, dest); toast("已导出加密备份。", "ok"); onClose(); }
    catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }
  return (
    <Modal title="导出加密备份" icon="ti-file-export" onClose={busy ? undefined : onClose}
      footer={<><button className="gh sm" disabled={busy} onClick={onClose}>取消</button><button className="pr sm" disabled={busy || !password} onClick={() => void submit()}>选择位置并导出</button></>}>
      <div className="vault-form">
        <div className="vault-sub">备份文件由当前主密码加密，可在其他电脑导入。</div>
        <label>主密码<input className="ip full" type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} /></label>
      </div>
    </Modal>
  );
}

function ImportDialog({ onClose, onChanged }: { onClose: () => void; onChanged: () => void }) {
  const toast = useToast();
  const [src, setSrc] = useState("");
  const [kind, setKind] = useState<Credential["kind"]>("password");
  const [value, setValue] = useState("");
  const [preview, setPreview] = useState<MergeStats | null>(null);
  const [busy, setBusy] = useState(false);
  const credential: Credential = { kind, value };

  async function choose() {
    const picked = await open({ title: "选择备份文件", multiple: false, directory: false, filters: BACKUP_FILTERS });
    if (typeof picked === "string") { setSrc(picked); setPreview(null); }
  }
  async function run(apply: boolean) {
    setBusy(true);
    try {
      const stats = apply ? await vaultApi.importApply(src, credential) : await vaultApi.importPreview(src, credential);
      if (apply) { toast(`已导入：新增 ${stats.added}，更新 ${stats.updated}。`, "ok"); onChanged(); onClose(); }
      else setPreview(stats);
    } catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }
  return (
    <Modal title="导入备份" icon="ti-file-import" onClose={busy ? undefined : onClose}
      footer={<>
        <button className="gh sm" disabled={busy} onClick={onClose}>取消</button>
        {preview
          ? <button className="pr sm" disabled={busy} onClick={() => void run(true)}>导入</button>
          : <button className="pr sm" disabled={busy || !src || !value} onClick={() => void run(false)}>预览</button>}
      </>}>
      <div className="vault-form">
        <div className="vault-sub">导入仅新增和更新条目，不会删除现有条目；同一条目保留较新的版本。</div>
        <div className="vault-bar"><button className="gh sm" disabled={busy} onClick={() => void choose()}><i className="ti ti-folder-open" /> 选择备份文件</button><span className="mut grow">{src}</span></div>
        <div className="seg">
          <button className={kind === "password" ? "on" : ""} onClick={() => { setKind("password"); setPreview(null); }}>备份的主密码</button>
          <button className={kind === "recovery" ? "on" : ""} onClick={() => { setKind("recovery"); setPreview(null); }}>备份的恢复密钥</button>
        </div>
        <input className="ip full" type="password" autoComplete="off" value={value} onChange={(e) => { setValue(e.target.value); setPreview(null); }} />
        {preview && <div className="callout"><i className="ti ti-info-circle" /><div>新增 {preview.added} / 更新 {preview.updated} / 相同 {preview.same}</div></div>}
      </div>
    </Modal>
  );
}

function AutoLockDialog({ onClose }: { onClose: () => void }) {
  const toast = useToast();
  const [minutes, setMinutes] = useState<number | null>(null);
  const [dirs, setDirs] = useState<string[]>([]);
  useEffect(() => {
    vaultApi.settings().then((settings) => { setMinutes(settings.vault_auto_lock_minutes); setDirs(settings.vault_scan_dirs); }).catch(() => setMinutes(10));
  }, []);
  async function choose(next: number) {
    try { await vaultApi.setSettings(next, dirs); setMinutes(next); toast("已保存。", "ok"); } catch (error) { toast(vaultError(error), "err"); }
  }
  return (
    <Modal title="自动锁定" icon="ti-clock-lock" sub="Stacker 窗口内无操作达到设定时长后锁定；Windows 锁屏或系统睡眠时也会锁定。" onClose={onClose}>
      <div className="seg">{AUTO_LOCK_CHOICES.map((value) => <button key={value} className={minutes === value ? "on" : ""} onClick={() => void choose(value)}>{value} 分钟</button>)}</div>
    </Modal>
  );
}
