import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

/** How far reading page titles has got, `[done, total]`, while `active`. */
export function useTitleProgress(active: boolean): [number, number] | null {
  const [progress, setProgress] = useState<[number, number] | null>(null);
  useEffect(() => {
    setProgress(null);
    if (!active) return;
    let stop: (() => void) | undefined;
    let disposed = false;
    void listen<[number, number]>("vault-titles-progress", (event) => { if (!disposed) setProgress(event.payload); })
      .then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; });
    return () => { disposed = true; stop?.(); };
  }, [active]);
  return progress;
}
