import { useState } from "react";
import { useToast } from "../../ui";
import { vaultApi, vaultError } from "./api";
import { PasswordForm } from "./PasswordForm";
import { RecoveryKeyStep } from "./RecoveryKeyStep";

/** Opened with the recovery key: new credentials first, entries only afterwards. */
export function VaultRecovering({ onChanged }: { onChanged: () => void }) {
  const toast = useToast();
  const [recoveryKey, setRecoveryKey] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function begin(password: string) {
    setBusy(true);
    try { setRecoveryKey(await vaultApi.recoverySetPassword(password)); }
    catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }
  async function cancel() {
    await vaultApi.cancelPending().catch(() => undefined);
    onChanged();
  }

  return (
    <div className="vault-center"><div className="pxcard">
      {recoveryKey ? (
        <RecoveryKeyStep recoveryKey={recoveryKey} onCancel={onChanged}
          onConfirmed={() => { toast("已启用新的主密码和恢复密钥。", "ok"); onChanged(); }} />
      ) : (
        <>
          <div className="vault-title"><i className="ti ti-lock-open" /> 设置新的主密码</div>
          <div className="vault-sub">解锁成功。请设置新的主密码，并保存新的恢复密钥。</div>
          <PasswordForm submitLabel="下一步" busy={busy} onSubmit={(password) => void begin(password)} />
          <div className="vault-links"><button className="vault-link" onClick={() => void cancel()}>取消</button></div>
        </>
      )}
    </div></div>
  );
}
