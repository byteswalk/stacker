import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useI18n } from "../../i18n";
import { useToast } from "../../ui";
import { formatSpaceBytes as bytes } from "./components/SpaceOverview";
import { driveName, lowSpaceStatus, type LowSpaceRun } from "./lowSpace";

const SEEN_KEY = "stacker.lowSpace.seen";

function seen(): number {
  try { return Number(localStorage.getItem(SEEN_KEY)) || 0; } catch { return 0; }
}

function markSeen(at: number) {
  try { localStorage.setItem(SEEN_KEY, String(Math.max(at, seen()))); } catch { /* the notice may show again */ }
}

/**
 * Says what a low-space cleanup did, wherever the user is. One that ran while the window was
 * hidden is told when it is shown again.
 */
export function useLowSpaceNotice() {
  const toast = useToast();
  const { tr } = useI18n();
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    const tell = (run: LowSpaceRun) => {
      const drive = driveName(run.drive);
      const text = run.freed > 0
        ? tr("{drive} 空间不足，已自动清理，释放 {size}。").replace("{drive}", drive).replace("{size}", bytes(run.freed))
        : tr("{drive} 空间不足，没有找到可以自动清理的内容。").replace("{drive}", drive);
      toast(text, run.freed > 0 ? "ok" : "info");
      markSeen(run.finishedAt);
    };
    const catchUp = () => {
      if (document.visibilityState !== "visible") return;
      lowSpaceStatus().then((status) => {
        if (disposed) return;
        const mark = seen();
        // The first time, only what happens from now on is told.
        if (!mark) { markSeen(status.runs[0]?.finishedAt ?? 1); return; }
        status.runs.filter((run) => run.finishedAt > mark).reverse().forEach(tell);
      }).catch(() => undefined);
    };
    catchUp();
    document.addEventListener("visibilitychange", catchUp);
    window.addEventListener("focus", catchUp);
    void listen<LowSpaceRun>("low-space-cleaned", (event) => {
      if (!disposed && document.visibilityState === "visible") tell(event.payload);
    }).then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; }).catch(() => undefined);
    return () => { disposed = true; stop?.(); document.removeEventListener("visibilitychange", catchUp); window.removeEventListener("focus", catchUp); };
  }, [toast, tr]);
}
