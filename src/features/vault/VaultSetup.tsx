import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../i18n";
import { useToast } from "../../ui";
import { vaultApi, vaultError } from "./api";
import { PasswordForm } from "./PasswordForm";
import { RecoveryKeyStep } from "./RecoveryKeyStep";
import { RetiredVaults, useRetiredVaults } from "./RetiredVaults";

export function VaultSetup({ onDone }: { onDone: () => void }) {
  const toast = useToast();
  const { tr } = useI18n();
  const [recoveryKey, setRecoveryKey] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const retired = useRetiredVaults();

  async function begin(password: string) {
    setBusy(true);
    try { setRecoveryKey(await vaultApi.createBegin(password)); }
    catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }
  async function restoreFrom(src: string) {
    try { await vaultApi.restoreBackup(src); toast("已从备份恢复，请使用该备份的主密码解锁。", "ok"); onDone(); }
    catch (error) { toast(vaultError(error), "err"); }
  }
  async function restore() {
    try {
      const src = await open({ title: tr("从备份文件恢复"), multiple: false, directory: false, filters: [{ name: tr("Stacker 保管库"), extensions: ["skv", "old"] }] });
      if (typeof src === "string") await restoreFrom(src);
    }
    catch (error) { toast(vaultError(error), "err"); }
  }

  return (
    <div className="vault-center"><div className="pxcard">
      {recoveryKey ? (
        <RecoveryKeyStep recoveryKey={recoveryKey} onCancel={() => setRecoveryKey(null)}
          onConfirmed={() => { toast("保管库已创建。", "ok"); onDone(); }} />
      ) : (
        <>
          <div className="vault-title"><i className="ti ti-shield-lock" /> 设置主密码</div>
          <div className="vault-sub">主密码用于加密保管库，不会保存在任何位置，请牢记。</div>
          <PasswordForm submitLabel="下一步" busy={busy} onSubmit={(password) => void begin(password)} />
          <div className="vault-links"><button className="vault-link" onClick={() => void restore()}>从备份文件恢复</button></div>
          <RetiredVaults retired={retired} pickLabel="恢复使用" pickIcon="ti-restore" onPick={(path) => void restoreFrom(path)} />
          {retired.length > 0 && <div className="vault-sub" style={{ margin: "8px 0 0" }}>「恢复使用」把旧保管库放回原处，用它原来的主密码或恢复密钥解锁。也可以先建新的保管库，再到「更多 → 导入备份」里把旧数据并进来。</div>}
        </>
      )}
    </div></div>
  );
}
