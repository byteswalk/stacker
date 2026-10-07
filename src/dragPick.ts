import { useEffect, useRef, type PointerEvent as ReactPointerEvent } from "react";

/**
 * Picking many rows of a list quickly: press a row's box and drag over other rows to set them
 * all the same way, or hold Shift to pick the range from the box pressed last. `rows` holds
 * the ids each visible row stands for, in the order shown (a group row may stand for several;
 * one that cannot be picked has none). The keyboard still ticks one box at a time.
 */
export function useDragPick<T>(rows: T[][], isPicked: (id: T) => boolean, set: (ids: T[], on: boolean) => void) {
  const dragging = useRef<boolean | null>(null);
  const last = useRef<number | null>(null);
  const pressed = useRef(false);
  // The handlers are made anew each render, so a drag that outlives a render reads the newest rows.
  useEffect(() => {
    // The click that follows a press clears `pressed`; should none come (the drag ended elsewhere), this does.
    const release = () => { dragging.current = null; window.setTimeout(() => { pressed.current = false; }, 500); };
    window.addEventListener("pointerup", release);
    window.addEventListener("pointercancel", release);
    return () => { window.removeEventListener("pointerup", release); window.removeEventListener("pointercancel", release); };
  }, []);

  function press(index: number, event: ReactPointerEvent) {
    const ids = rows[index] ?? [];
    if (event.button !== 0 || ids.length === 0) return;
    // No text selection while dragging; the click that follows is swallowed by `change`.
    event.preventDefault();
    pressed.current = true;
    const on = !ids.every(isPicked);
    const range = event.shiftKey && last.current !== null;
    const from = range ? Math.min(last.current!, index) : index;
    const to = range ? Math.max(last.current!, index) : index;
    set(rows.slice(from, to + 1).flat(), on);
    last.current = index;
    dragging.current = on;
  }

  return {
    /** On the box, or the cell around it: where a press starts picking. */
    box: (index: number) => ({ onPointerDown: (event: ReactPointerEvent) => press(index, event) }),
    /** On the whole row: a drag passing over it picks it too. */
    row: (index: number) => ({
      onPointerEnter: () => {
        if (dragging.current === null) return;
        const ids = rows[index] ?? [];
        if (ids.length) set(ids, dragging.current);
      },
    }),
    /** The box's onChange: a keyboard tick; a press has already done its part. */
    change: (index: number, on: boolean) => {
      if (pressed.current) { pressed.current = false; return; }
      const ids = rows[index] ?? [];
      if (ids.length) set(ids, on);
      last.current = index;
    },
  };
}

/** `set` for a picked list kept as an array. */
export const setInArray = <T,>(old: T[], ids: T[], on: boolean): T[] =>
  on ? [...old, ...ids.filter((id) => !old.includes(id))] : old.filter((id) => !ids.includes(id));

/** `set` for a picked list kept as a Set. */
export const setInSet = <T,>(old: Set<T>, ids: T[], on: boolean): Set<T> => {
  const next = new Set(old);
  for (const id of ids) { if (on) next.add(id); else next.delete(id); }
  return next;
};
