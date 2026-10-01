import type { EntryView } from "./api";
import { KIND_LABELS } from "./labels";
import { daysUntil, expiryState, formatTime } from "./vaultView";

export function ExpiryBadge({ expiresAt, today }: { expiresAt: string | null; today: Date }) {
  const state = expiryState(expiresAt, today);
  if (!expiresAt || state === "none") return <span className="mut">—</span>;
  if (state === "expired") return <span className="vault-badge expired">已到期</span>;
  if (state === "soon") return <span className="vault-badge soon">{daysUntil(expiresAt, today)} 天后到期</span>;
  return <span className="mut">{expiresAt}</span>;
}

export function EntryList({ entries, selectedId, today, onSelect }: {
  entries: EntryView[]; selectedId: string | null; today: Date; onSelect: (id: string) => void;
}) {
  if (entries.length === 0) return <div className="vault-empty">没有符合条件的条目。</div>;
  return (
    <div className="vault-rows" role="list">
      {entries.map((entry) => (
        <button key={entry.id} role="listitem" className={"vault-row" + (entry.id === selectedId ? " on" : "")} onClick={() => onSelect(entry.id)}>
          <i className={"ti " + (entry.favorite ? "ti-star-filled" : "ti-key")} aria-hidden="true" />
          <span translate="no">{entry.title}</span>
          <span className="mut">{entry.platform ? `${entry.platform} · ${KIND_LABELS[entry.kind]}` : KIND_LABELS[entry.kind]}</span>
          <ExpiryBadge expiresAt={entry.expiresAt} today={today} />
          <span className="mut">{formatTime(entry.updatedAt).slice(0, 10)}</span>
        </button>
      ))}
    </div>
  );
}
