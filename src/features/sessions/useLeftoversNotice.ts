import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useI18n } from "../../i18n";
import { useToast } from "../../ui";
import { formatSpaceBytes as bytes } from "../space-analysis/components/SpaceOverview";

/** Says what the once-a-day leftover cleanup freed, wherever the user is. */
export function useLeftoversNotice() {
  const toast = useToast();
  const { tr } = useI18n();
  useEffect(() => {
    let stop: (() => void) | undefined;
    let disposed = false;
    void listen<number>("leftovers-auto-cleaned", (event) => {
      if (disposed || !event.payload) return;
      toast(tr("已自动清理会话残留，释放 {size}。").replace("{size}", bytes(event.payload)), "ok");
    }).then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; });
    return () => { disposed = true; stop?.(); };
  }, [toast, tr]);
}
