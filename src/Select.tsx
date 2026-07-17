import { useEffect, useId, useRef, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";

export type SelOption = { value: string; label: ReactNode; disabled?: boolean; title?: string };
export type SelGroup = { label?: string; options: SelOption[] };

/** 自定义深色下拉：替代原生 <select>，统一深/浅主题下的 hover / 选中样式。
 *  菜单用 position:fixed（按触发器位置定位），避免被滚动容器裁切；点外部 / 外部滚动 / Esc 关闭。
 *  传 options（扁平）或 groups（带分组标题）。 */
export function Select({ value, onChange, options, groups, disabled, placeholder, className, width }: {
  value: string;
  onChange: (v: string) => void;
  options?: SelOption[];
  groups?: SelGroup[];
  disabled?: boolean;
  placeholder?: string;
  className?: string;
  width?: number | string;
}) {
  const grps: SelGroup[] = groups ?? [{ options: options ?? [] }];
  const all = grps.flatMap((g) => g.options);
  const cur = all.find((o) => o.value === value);
  const labelTitle = (o?: SelOption) => o?.title ?? (typeof o?.label === "string" ? o.label : undefined);
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);
  const [pos, setPos] = useState<{ left: number; top?: number; bottom?: number; width: number; maxHeight: number } | null>(null);
  const trig = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const menuId = useId();
  const enabled = all.filter((option) => !option.disabled);

  function place() {
    const el = trig.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const margin = 8;
    const gap = 4;
    const optionCount = grps.reduce((sum, g) => sum + g.options.length + (g.label ? 1 : 0), 0);
    const desiredHeight = Math.min(300, Math.max(44, optionCount * 34 + 8));
    const below = window.innerHeight - r.bottom - margin;
    const above = r.top - margin;
    const openUp = below < desiredHeight && above > below;
    const maxHeight = Math.max(96, Math.min(300, (openUp ? above : below) - gap));
    const left = Math.max(margin, Math.min(r.left, window.innerWidth - r.width - margin));
    setPos(openUp
      ? { left, bottom: window.innerHeight - r.top + gap, width: r.width, maxHeight }
      : { left, top: r.bottom + gap, width: r.width, maxHeight });
  }
  function toggle() {
    if (disabled) return;
    if (!open) {
      place();
      const index = enabled.findIndex((option) => option.value === value);
      setActiveIndex(index >= 0 ? index : 0);
    }
    setOpen((o) => !o);
  }
  function pick(o: SelOption) { if (o.disabled) return; onChange(o.value); setOpen(false); }
  function onTriggerKeyDown(e: ReactKeyboardEvent<HTMLButtonElement>) {
    if (disabled) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) {
        place();
        setOpen(true);
      }
      const delta = e.key === "ArrowDown" ? 1 : -1;
      setActiveIndex((current) => {
        if (!enabled.length) return -1;
        const start = current < 0 ? (delta > 0 ? -1 : 0) : current;
        return (start + delta + enabled.length) % enabled.length;
      });
    } else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      if (!open) toggle();
      else if (activeIndex >= 0 && enabled[activeIndex]) pick(enabled[activeIndex]);
    } else if (e.key === "Escape" && open) {
      e.preventDefault();
      setOpen(false);
    }
  }

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node;
      if (trig.current?.contains(t) || menu.current?.contains(t)) return;
      setOpen(false);
    };
    const onScroll = (e: Event) => {
      const t = e.target;
      if (t instanceof Node && (trig.current?.contains(t) || menu.current?.contains(t))) return;
      setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("mousedown", onDown);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onScroll);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onScroll);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <>
      <button ref={trig} type="button" disabled={disabled} onClick={toggle} onKeyDown={onTriggerKeyDown}
        aria-haspopup="listbox" aria-expanded={open} aria-controls={open ? menuId : undefined}
        aria-activedescendant={open && activeIndex >= 0 ? `${menuId}-option-${activeIndex}` : undefined}
        className={"sel2" + (open ? " open" : "") + (className ? " " + className : "")}
        title={labelTitle(cur)}
        style={width ? { width } : undefined}>
        <span className="sel2v">{cur ? cur.label : <span className="sel2ph">{placeholder ?? "请选择"}</span>}</span>
        <i className="ti ti-chevron-down sel2c" />
      </button>
      {open && pos && (
        <div ref={menu} id={menuId} role="listbox" className="selmenu" style={{
          left: pos.left,
          top: pos.top,
          bottom: pos.bottom,
          minWidth: pos.width,
          maxHeight: pos.maxHeight,
          maxWidth: `calc(100vw - ${pos.left + 8}px)`,
        }}>
          {grps.map((g, gi) => (
            <div key={gi} role={g.label ? "group" : undefined} aria-label={g.label}>
              {g.label && <div className="selgl">{g.label}</div>}
              {g.options.map((o) => {
                const enabledIndex = enabled.indexOf(o);
                return (
                <button key={o.value} id={enabledIndex >= 0 ? `${menuId}-option-${enabledIndex}` : undefined}
                  role="option" aria-selected={o.value === value} tabIndex={-1} type="button" disabled={o.disabled}
                  title={labelTitle(o)}
                  className={"selopt" + (o.value === value ? " on" : "") + (enabledIndex === activeIndex ? " active" : "")}
                  onMouseEnter={() => { if (enabledIndex >= 0) setActiveIndex(enabledIndex); }} onClick={() => pick(o)}>
                  <span className="selopt-l">{o.label}</span>
                  {o.value === value && <i className="ti ti-check selck" />}
                </button>
              );})}
            </div>
          ))}
        </div>
      )}
    </>
  );
}
