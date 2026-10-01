import { useCallback, useEffect, useRef, useState } from "react";

export const REVEAL_MS = 30_000;

/** Values shown on request; each hides itself after 30 seconds and all go when the view unmounts. */
export function useRevealed() {
  const [revealed, setRevealed] = useState<Record<string, string>>({});
  const timers = useRef(new Map<string, number>());

  const hide = useCallback((key: string) => {
    window.clearTimeout(timers.current.get(key));
    timers.current.delete(key);
    setRevealed((current) => {
      const next = { ...current };
      delete next[key];
      return next;
    });
  }, []);

  const show = useCallback((key: string, value: string) => {
    window.clearTimeout(timers.current.get(key));
    timers.current.set(key, window.setTimeout(() => hide(key), REVEAL_MS));
    setRevealed((current) => ({ ...current, [key]: value }));
  }, [hide]);

  useEffect(() => {
    const active = timers.current;
    return () => { active.forEach((timer) => window.clearTimeout(timer)); active.clear(); };
  }, []);

  return { revealed, show, hide };
}
