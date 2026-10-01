import { useState } from "react";
import { ConfirmModal, useToast } from "../../ui";
import { vaultApi, vaultError, type VaultStatus } from "./api";

export function VaultUnlock({ status, onChanged }: { status: VaultStatus; onChanged: () => void }) {
  const toast = useToast();
  const [mode, setMode] = useState<"password" | "recovery">("password");
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [resetting, setResetting] = useState(false);

  async function submit() {
    setBusy(true);
    try {
      if (mode === "password") await vaultApi.unlock(value);
      else await vaultApi.unlockRecovery(value);
      setValue("");
      onChanged();
    } catch (error) {
      toast(vaultError(error), "err");
      onChanged();
    } finally { setBusy(false); }
  }
  async function reset() {
    setResetting(false);
    try { const kept = await vaultApi.reset(); toast(`已改名保留原保管库：${kept}`, "ok"); onChanged(); }
    catch (error) { toast(vaultError(error), "err"); }
  }

  return (
    <div className="vault-center"><div className="pxcard">
      <div className="vault-title"><i className="ti ti-lock" /> {mode === "password" ? "解锁保管库" : "使用恢复密钥解锁"}</div>
      <form className="vault-form" onSubmit={(event) => { event.preventDefault(); void submit(); }}>
        <label>{mode === "password" ? "主密码" : "恢复密钥"}
          <input className="ip full" type="password" autoComplete={mode === "password" ? "current-password" : "off"} autoFocus value={value} onChange={(e) => setValue(e.target.value)} />
        </label>
        {status.waitSeconds > 0 && <div className="vault-warn">尝试次数过多，请 {status.waitSeconds} 秒后再试。</div>}
        <div className="vault-actions"><button className="pr sm" type="submit" disabled={busy || !value}>解锁</button></div>
      </form>
      <div className="vault-links">
        <button className="vault-link" onClick={() => { setMode(mode === "password" ? "recovery" : "password"); setValue(""); }}>
          {mode === "password" ? "使用恢复密钥" : "使用主密码"}
        </button>
        <button className="vault-link" onClick={() => setResetting(true)}>无法解锁？</button>
      </div>
      {resetting && (
        <ConfirmModal title="重置保管库" danger confirmLabel="重置"
          message="现有保管库将改名保留，并创建一个新的空保管库。日后想起主密码或恢复密钥，可通过导入找回旧数据。"
          onClose={() => setResetting(false)} onConfirm={() => void reset()} />
      )}
    </div></div>
  );
}
