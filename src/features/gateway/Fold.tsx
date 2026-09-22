import { useState, type ReactNode } from "react";

const KEY = "stacker.gateway.open";

function readOpen(): Record<string, boolean> {
  try { return JSON.parse(localStorage.getItem(KEY) || "{}") as Record<string, boolean>; } catch { return {}; }
}

/** Open or closed, remembered per viewer; the default applies until they choose. */
export function useFold(id: string, openByDefault: boolean): [boolean, () => void] {
  const [open, setOpen] = useState(() => readOpen()[id] ?? openByDefault);
  const toggle = () => {
    const next = !open;
    setOpen(next);
    try { localStorage.setItem(KEY, JSON.stringify({ ...readOpen(), [id]: next })); } catch { /* per-viewer convenience only */ }
  };
  return [open, toggle];
}

/** A chevron that opens and closes a section, for the section's own header row. */
export function FoldToggle({ open, onToggle, label }: { open: boolean; onToggle: () => void; label: string }) {
  return (
    <button type="button" className="gw-fold" aria-expanded={open} aria-label={label} title={label} onClick={onToggle}>
      <i className={"ti " + (open ? "ti-chevron-down" : "ti-chevron-right")} />
    </button>
  );
}

/** A card whose body opens from its header. */
export function FoldCard({ id, openByDefault = false, title, children }: {
  id: string; openByDefault?: boolean; title: ReactNode; children: ReactNode;
}) {
  const [open, toggle] = useFold(id, openByDefault);
  return (
    <div className={"pxcard gw-foldcard" + (open ? " open" : "")}>
      <button type="button" className="pxsec gw-foldhead" aria-expanded={open} onClick={toggle}>
        <i className={"ti gw-chevron " + (open ? "ti-chevron-down" : "ti-chevron-right")} />{title}
      </button>
      {open && <div className="gw-foldbody">{children}</div>}
    </div>
  );
}
