import { useEffect, useRef, useState, type ReactNode } from "react";

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
  const [pos, setPos] = useState<{ left: number; top: number; width: number } | null>(null);
  const trig = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);

  function place() {
    const el = trig.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos({ left: r.left, top: r.bottom + 4, width: r.width });
  }
  function toggle() { if (disabled) return; if (!open) place(); setOpen((o) => !o); }
  function pick(o: SelOption) { if (o.disabled) return; onChange(o.value); setOpen(false); }

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
      <button ref={trig} type="button" disabled={disabled} onClick={toggle}
        className={"sel2" + (open ? " open" : "") + (className ? " " + className : "")}
        title={labelTitle(cur)}
        style={width ? { width } : undefined}>
        <span className="sel2v">{cur ? cur.label : <span className="sel2ph">{placeholder ?? "请选择"}</span>}</span>
        <i className="ti ti-chevron-down sel2c" />
      </button>
      {open && pos && (
        <div ref={menu} className="selmenu" style={{ left: pos.left, top: pos.top, minWidth: pos.width }}>
          {grps.map((g, gi) => (
            <div key={gi}>
              {g.label && <div className="selgl">{g.label}</div>}
              {g.options.map((o) => (
                <button key={o.value} type="button" disabled={o.disabled}
                  title={labelTitle(o)}
                  className={"selopt" + (o.value === value ? " on" : "")} onClick={() => pick(o)}>
                  <span className="selopt-l">{o.label}</span>
                  {o.value === value && <i className="ti ti-check selck" />}
                </button>
              ))}
            </div>
          ))}
        </div>
      )}
    </>
  );
}
