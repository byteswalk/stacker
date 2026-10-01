import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useToast } from "../../ui";
import { vaultApi } from "./api";
import { lockReasonText } from "./labels";

/** Explains why the vault locked itself, on whichever page the user happens to be. */
export function useVaultLockNotice() {
  const toast = useToast();
  useEffect(() => {
    let stop: (() => void) | undefined;
    let disposed = false;
    void listen<string>("vault-locked", async (event) => {
      const minutes = await vaultApi.settings().then((settings) => settings.vault_auto_lock_minutes).catch(() => 10);
      if (!disposed) toast(lockReasonText(event.payload, minutes), "info");
    }).then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; });
    return () => { disposed = true; stop?.(); };
  }, [toast]);
}
