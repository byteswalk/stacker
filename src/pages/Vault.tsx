import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ErrorState, Loading } from "../ui";
import { vaultApi, type VaultStatus } from "../features/vault/api";
import { VaultRecovering } from "../features/vault/VaultRecovering";
import { VaultSetup } from "../features/vault/VaultSetup";
import { VaultUnlock } from "../features/vault/VaultUnlock";
import { VaultWorkspace } from "../features/vault/VaultWorkspace";
import "../features/vault/vault.css";

export default function Vault() {
  const [status, setStatus] = useState<VaultStatus | null>(null);
  const [failed, setFailed] = useState(false);

  const reload = useCallback(async () => {
    try { setStatus(await vaultApi.status()); setFailed(false); } catch { setFailed(true); }
  }, []);
  useEffect(() => { void reload(); }, [reload]);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let disposed = false;
    // The lock-reason toast lives in useVaultLockNotice (App shell); the page only refreshes its state.
    void listen<string>("vault-locked", () => { void reload(); })
      .then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; });
    return () => { disposed = true; stop?.(); };
  }, [reload]);

  if (failed) return <ErrorState title="暂时无法读取保管库状态" description="请稍后重试。" onRetry={reload} />;
  if (!status) return <Loading text="正在读取保管库状态…" />;
  if (status.state === "missing") return <VaultSetup onDone={reload} />;
  if (status.state === "locked") return <VaultUnlock status={status} onChanged={reload} />;
  if (status.state === "recovering") return <VaultRecovering onChanged={reload} />;
  return <VaultWorkspace onLocked={reload} />;
}
