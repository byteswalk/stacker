import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ErrorState, Loading, useToast } from "../ui";
import { vaultApi, type VaultStatus } from "../features/vault/api";
import { lockReasonText } from "../features/vault/labels";
import { VaultRecovering } from "../features/vault/VaultRecovering";
import { VaultSetup } from "../features/vault/VaultSetup";
import { VaultUnlock } from "../features/vault/VaultUnlock";
import { VaultWorkspace } from "../features/vault/VaultWorkspace";
import "../features/vault/vault.css";

export default function Vault() {
  const toast = useToast();
  const [status, setStatus] = useState<VaultStatus | null>(null);
  const [failed, setFailed] = useState(false);

  const reload = useCallback(async () => {
    try { setStatus(await vaultApi.status()); setFailed(false); } catch { setFailed(true); }
  }, []);
  useEffect(() => { void reload(); }, [reload]);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let disposed = false;
    void listen<string>("vault-locked", async (event) => {
      const minutes = await vaultApi.settings().then((settings) => settings.vault_auto_lock_minutes).catch(() => 10);
      toast(lockReasonText(event.payload, minutes), "info");
      void reload();
    }).then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; });
    return () => { disposed = true; stop?.(); };
  }, [reload, toast]);

  if (failed) return <ErrorState title="暂时无法读取保管库状态" description="请稍后重试。" onRetry={reload} />;
  if (!status) return <Loading text="正在读取保管库状态…" />;
  if (status.state === "missing") return <VaultSetup onDone={() => void reload()} />;
  if (status.state === "locked") return <VaultUnlock status={status} onChanged={() => void reload()} />;
  if (status.state === "recovering") return <VaultRecovering onChanged={() => void reload()} />;
  return <VaultWorkspace onLocked={() => void reload()} />;
}
