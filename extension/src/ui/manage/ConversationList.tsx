import { useState } from "react";
import { t } from "../../i18n";
import { bodyIsFresh, type Conversation } from "../../lib/db";
import { SITES } from "../../sites/registry";

const PAGE = 50;

export function ConversationList({ items, aliasOf, selected, onSelect, onOpen, active }: {
  items: Conversation[]; aliasOf: (key: string) => string; selected: Set<string>;
  onSelect: (keys: string[], on: boolean) => void; onOpen: (c: Conversation) => void; active: string | null;
}) {
  const [page, setPage] = useState(0);
  const pages = Math.max(1, Math.ceil(items.length / PAGE));
  const current = Math.min(page, pages - 1);
  const shown = items.slice(current * PAGE, current * PAGE + PAGE);
  const allShown = shown.length > 0 && shown.every((c) => selected.has(c.key));
  return <>
    <div className="row">
      <label><input type="checkbox" checked={allShown} onChange={(e) => onSelect(shown.map((c) => c.key), e.target.checked)} /> {t("本页")}</label>
      <button disabled={!items.length} onClick={() => onSelect(items.map((c) => c.key), true)}>{t("选中全部结果")}（{items.length}）</button>
      {selected.size > 0 && <button onClick={() => onSelect([...selected], false)}>{t("取消选择")}</button>}
      <span className="mut">{t("已选")} {selected.size}</span>
    </div>
    <ul className="list">
      {shown.map((c) => <li key={c.key} className={active === c.key ? "active" : ""} onClick={() => onOpen(c)}>
        <input type="checkbox" checked={selected.has(c.key)} onClick={(e) => e.stopPropagation()} onChange={(e) => onSelect([c.key], e.target.checked)} />
        <div>
          <div>{c.favorite ? "★ " : ""}{c.title || t("（无标题）")}{c.removedAt !== null && <span className="mut"> · {t("已删除")}</span>}</div>
          <div className="mut">{SITES[c.site].label} · {aliasOf(c.account)} · {new Date(c.updatedAt).toLocaleString()}
            {bodyIsFresh(c) && <> · {t("正文已读")}</>}</div>
          <div>{c.tags.map((tag) => <span key={tag} className="chip">{tag}</span>)}</div>
        </div>
        <span className="mut">{c.archived ? t("已归档") : ""}</span>
      </li>)}
    </ul>
    {pages > 1 && <div className="row">
      <button disabled={current === 0} onClick={() => setPage(current - 1)}>‹</button>
      <span className="mut">{current + 1} / {pages}</span>
      <button disabled={current >= pages - 1} onClick={() => setPage(current + 1)}>›</button>
    </div>}
  </>;
}
