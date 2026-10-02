import { useState } from "react";
import { ConfirmModal, useToast } from "../../ui";
import { vaultApi, vaultError, type VaultStatus } from "./api";
import { emptyKey, keyComplete, keyText, RecoveryKeyInput } from "./RecoveryKeyInput";

export function VaultUnlock({ status, onChanged }: { status: VaultStatus; onChanged: () => void }) {
  const toast = useToast();
  const [mode, setMode] = useState<"password" | "recovery">("password");
  const [value, setValue] = useState("");
  const [groups, setGroups] = useState(emptyKey);
  const [busy, setBusy] = useState(false);
  const [resetting, setResetting] = useState(false);
  const ready = mode === "password" ? value !== "" : keyComplete(groups);

  async function submit() {
    setBusy(true);
    try {
      if (mode === "password") await vaultApi.unlock(value);
      else await vaultApi.unlockRecovery(keyText(groups));
      setValue("");
      setGroups(emptyKey());
      onChanged();
    } catch (error) {
      toast(vaultError(error), "err");
      onChanged();
    } finally { setBusy(false); }
  }
  // The setup page that follows lists the file that was set aside, so the path is not lost with the toast.
  async function reset() {
    setResetting(false);
    try { await vaultApi.reset(); toast("原保管库已改名保留，位置见下方。", "ok"); onChanged(); }
    catch (error) { toast(vaultError(error), "err"); }
  }

  return (
    <div className={"vault-center" + (mode === "recovery" ? " wide" : "")}><div className="pxcard">
      <div className="vault-title"><i className="ti ti-lock" /> {mode === "password" ? "解锁保管库" : "使用恢复密钥解锁"}</div>
      <form className="vault-form" onSubmit={(event) => { event.preventDefault(); if (ready) void submit(); }}>
        {mode === "password"
          ? <label>主密码
            <input className="ip full" type="password" autoComplete="current-password" autoFocus value={value} onChange={(e) => setValue(e.target.value)} />
          </label>
          : <RecoveryKeyInput groups={groups} onChange={setGroups} disabled={busy} autoFocus />}
        {status.waitSeconds > 0 && <div className="vault-warn">尝试次数过多，请 {status.waitSeconds} 秒后再试。</div>}
        <div className="vault-actions"><button className="pr sm" type="submit" disabled={busy || !ready}>解锁</button></div>
      </form>
      <div className="vault-links">
        <button className="vault-link" onClick={() => { setMode(mode === "password" ? "recovery" : "password"); setValue(""); setGroups(emptyKey()); }}>
          {mode === "password" ? "使用恢复密钥" : "使用主密码"}
        </button>
        <button className="vault-link" onClick={() => setResetting(true)}>无法解锁？</button>
      </div>
      {resetting && (
        <ConfirmModal title="重置保管库" danger confirmLabel="重置"
          message="现有保管库不会删除，只是改名留在原文件夹里，然后让你新建一个空保管库。重置后的页面会列出这个旧文件：想起主密码或恢复密钥时，可以把它恢复使用，或导入到新保管库。"
          onClose={() => setResetting(false)} onConfirm={() => void reset()} />
      )}
    </div></div>
  );
}
