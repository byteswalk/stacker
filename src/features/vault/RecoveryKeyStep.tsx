import { useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../i18n";
import { useToast } from "../../ui";
import { vaultApi, vaultError } from "./api";

export function RecoveryKeyStep({ recoveryKey, onConfirmed, onCancel }: { recoveryKey: string; onConfirmed: () => void; onCancel: () => void }) {
  const { tr } = useI18n();
  const toast = useToast();
  const [group, setGroup] = useState("");
  const [savedName, setSavedName] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // Copying goes through the backend clipboard (excluded from Win+V history), never navigator.clipboard.
  async function copy() {
    try { await vaultApi.copyRecovery(); toast("已复制，30 秒后自动清除剪贴板。", "ok"); } catch (error) { toast(vaultError(error), "err"); }
  }
  async function saveFile() {
    try {
      const today = new Date().toLocaleDateString("sv");
      const path = await save({ title: tr("存为文件"), defaultPath: `${tr("Stacker恢复密钥")}-${today}.txt`, filters: [{ name: tr("文本文件"), extensions: ["txt"] }] });
      if (!path) return;
      setSavedName(await vaultApi.saveRecovery(path, tr("这是 Stacker 密钥保管库的恢复密钥。忘记主密码时用它解锁保管库。此文件没有加密，请妥善保管。")));
    } catch (error) { toast(vaultError(error), "err"); }
  }
  // The backend discards the pending key on any failure except a wrong last group, so every error is just toasted.
  async function confirm() {
    setBusy(true);
    try { await vaultApi.confirmRecovery(group, savedName); onConfirmed(); }
    catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }
  async function cancel() {
    await vaultApi.cancelPending().catch(() => undefined);
    onCancel();
  }

  return (
    <div>
      <div className="vault-title"><i className="ti ti-lifebuoy" /> 保存恢复密钥</div>
      <div className="vault-sub">忘记主密码时，可使用恢复密钥解锁保管库。恢复密钥仅显示一次；主密码与恢复密钥均丢失时，数据无法恢复。</div>
      <div className="vault-key" translate="no">{recoveryKey}</div>
      <div className="vault-actions" style={{ justifyContent: "center", marginTop: 0 }}>
        <button className="gh sm" onClick={() => void copy()}><i className="ti ti-copy" /> 复制</button>
        <button className="gh sm" onClick={() => void saveFile()}><i className="ti ti-file-download" /> 存为文件</button>
      </div>
      {savedName && <div className="vault-sub">已存为 <span className="vault-keyfile" translate="no">{savedName}</span>。文件没有加密，请放到保管库之外的安全位置；使用恢复密钥时，Stacker 会提示这个文件名。</div>}
      <div className="vault-sub">保存至微信收藏、手机备忘录或其他密码管理器，或打印后妥善存放。</div>
      <div className="vault-warn">请勿与保管库文件存放在同一位置。</div>
      <div className="vault-form" style={{ marginTop: 12 }}>
        <label>请输入恢复密钥的最后一组字符，以确认已保存。
          <input className="ip" style={{ width: 120 }} maxLength={4} autoComplete="off" value={group} onChange={(e) => setGroup(e.target.value)} />
        </label>
      </div>
      <div className="vault-actions">
        <button className="gh sm" disabled={busy} onClick={() => void cancel()}>取消</button>
        <button className="pr sm" disabled={busy || group.trim().length !== 4} onClick={() => void confirm()}>完成</button>
      </div>
    </div>
  );
}
