import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useI18n } from "../../i18n";
import { useToast } from "../../ui";
import { vaultApi } from "./api";
import { lockReasonText } from "./labels";

/** Explains why the vault locked itself, on whichever page the user happens to be. */
export function useVaultLockNotice() {
  const toast = useToast();
  const { tr } = useI18n();
  useEffect(() => {
    let stop: (() => void) | undefined;
    let disposed = false;
    void listen<string>("vault-locked", async (event) => {
      const minutes = await vaultApi.settings().then((settings) => settings.vault_auto_lock_minutes).catch(() => 10);
      if (!disposed) toast(lockReasonText(event.payload, minutes), "info");
    }).then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; });
    // Logins the browser extension saved, taken into the vault while it was open.
    let stopInbox: (() => void) | undefined;
    void listen<number>("vault-inbox", (event) => {
      if (!disposed) toast(tr("已从浏览器收进 {count} 条登录。").replace("{count}", String(event.payload)), "ok");
    }).then((unlisten) => { if (disposed) unlisten(); else stopInbox = unlisten; });
    return () => { disposed = true; stop?.(); stopInbox?.(); };
  }, [toast, tr]);
}
