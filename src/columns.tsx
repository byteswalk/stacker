import { useState, type PointerEvent as ReactPointerEvent } from "react";
import { useI18n } from "./i18n";

/**
 * One column of a grid list. `track` is its grid track while left alone. A `resizable` one
 * can be dragged between `min` and the room the others can give; dragged, it becomes at most
 * that wide and shrinks with the window, so the list never runs wider than its box. The one
 * column that `grows` (the actions) is never dragged: it takes whatever room is left once
 * no other column stretches, and keeps `min` pixels.
 */
export type Column = { key: string; track: string; resizable?: boolean; min?: number; grows?: boolean };

const MIN = 60;
const MAX = 900;

function read(storeKey: string): Record<string, number> {
  try {
    const value = JSON.parse(localStorage.getItem(storeKey) ?? "{}") as unknown;
    return value && typeof value === "object" ? value as Record<string, number> : {};
  } catch { return {}; }
}

/** The grid template for these columns with these dragged widths. */
export function templateOf(columns: Column[], widths: Record<string, number>): string {
  const dragged = (column: Column) => !!column.resizable && widths[column.key] > 0;
  const stretches = columns.some((column) => !column.grows && !dragged(column) && column.track.includes("fr"));
  return columns.map((column) => {
    if (dragged(column)) return `minmax(${column.min ?? MIN}px, ${widths[column.key]}px)`;
    if (column.grows && !stretches) return `minmax(${column.track}, 1fr)`;
    return column.track;
  }).join(" ");
}

/** Drag handles for a grid list's header, with widths kept per viewer under `storeKey`. */
export function useColumns(storeKey: string, columns: Column[]) {
  const { tr } = useI18n();
  const [widths, setWidths] = useState<Record<string, number>>(() => read(storeKey));
  const keep = (next: Record<string, number>) => {
    setWidths(next);
    try { localStorage.setItem(storeKey, JSON.stringify(next)); } catch { /* a per-viewer convenience only */ }
  };

  function start(key: string, event: ReactPointerEvent<HTMLElement>) {
    event.preventDefault();
    event.stopPropagation();
    const row = event.currentTarget.closest<HTMLElement>("[data-columns]");
    if (!row) return;
    // What the grid actually gave each column, and what the others could still give up.
    const tracks = getComputedStyle(row).gridTemplateColumns.split(" ").map((value) => parseFloat(value) || 0);
    const style = getComputedStyle(row);
    const inner = row.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
    const gap = parseFloat(style.columnGap) || 0;
    const used = tracks.reduce((sum, value) => sum + value, 0) + gap * Math.max(0, tracks.length - 1);
    const index = columns.findIndex((column) => column.key === key);
    const own = tracks[index] ?? 0;
    const slack = Math.max(0, inner - used) + columns.reduce((sum, column, at) => {
      if (at === index) return sum;
      if (column.resizable) return sum + Math.max(0, (tracks[at] ?? 0) - (column.min ?? MIN));
      if (column.grows) return sum + Math.max(0, (tracks[at] ?? 0) - (column.min ?? 0));
      return sum;
    }, 0);
    const lowest = columns[index]?.min ?? MIN;
    const highest = Math.min(MAX, own + slack);
    const from = event.clientX;
    let latest = widths;
    const move = (moved: PointerEvent) => {
      latest = { ...widths, [key]: Math.round(Math.max(lowest, Math.min(highest, own + moved.clientX - from))) };
      setWidths(latest);
    };
    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      keep(latest);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  }

  return {
    /** The grid template every row of the list uses; the header row also carries `data-columns`. */
    template: templateOf(columns, widths),
    /** The handle on a header cell's right edge; double-click puts the column back. */
    handle: (key: string) => <i className="col-resize" title={tr("拖动调整列宽，双击恢复")}
      onPointerDown={(event) => start(key, event)}
      onDoubleClick={() => { const next = { ...widths }; delete next[key]; keep(next); }} />,
  };
}
