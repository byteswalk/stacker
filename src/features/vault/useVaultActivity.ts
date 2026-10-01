import { useEffect } from "react";
import { vaultApi } from "./api";

const PING_EVERY_MS = 30_000;

/** Tells the vault the user is active in Stacker; idle locking counts from the last ping. */
export function useVaultActivity() {
  useEffect(() => {
    let last = 0;
    const ping = () => {
      const now = Date.now();
      if (now - last < PING_EVERY_MS) return;
      last = now;
      void vaultApi.touch().catch(() => undefined);
    };
    const events = ["pointerdown", "keydown", "wheel"] as const;
    events.forEach((name) => window.addEventListener(name, ping, { passive: true }));
    return () => events.forEach((name) => window.removeEventListener(name, ping));
  }, []);
}
