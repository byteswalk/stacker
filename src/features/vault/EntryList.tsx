import { useState } from "react";
import { ConfirmModal, useToast } from "../../ui";
import { vaultApi, vaultError, type EntryView } from "./api";
import { KIND_LABELS } from "./labels";
import { daysUntil, expiryState, formatTime } from "./vaultView";

export function ExpiryBadge({ expiresAt, today }: { expiresAt: string | null; today: Date }) {
  const state = expiryState(expiresAt, today);
  if (!expiresAt || state === "none") return <span className="mut">—</span>;
  if (state === "expired") return <span className="vault-badge expired">已到期</span>;
  if (state === "soon") return <span className="vault-badge soon">{daysUntil(expiresAt, today)} 天后到期</span>;
  return <span className="mut">{expiresAt}</span>;
}

/** The field a row's copy button takes: the first secret that has a value, or else the first value. */
export function mainField(entry: EntryView): string | null {
  const filled = entry.fields.filter((field) => field.filled);
  return (filled.find((field) => field.secret) ?? filled[0])?.name ?? null;
}

export function EntryList({ entries, today, onView, onEdit, onChanged }: {
  entries: EntryView[]; today: Date; onView: (entry: EntryView) => void; onEdit: (entry: EntryView) => void; onChanged: () => void;
}) {
  const toast = useToast();
  const [deleting, setDeleting] = useState<EntryView | null>(null);
  const [busy, setBusy] = useState(false);

  async function copy(entry: EntryView, field: string) {
    try { await vaultApi.copy(entry.id, field); toast("已复制，30 秒后自动清除剪贴板。", "ok"); } catch (error) { toast(vaultError(error), "err"); }
  }
  async function remove(entry: EntryView) {
    setBusy(true);
    try { await vaultApi.remove(entry.id); toast("已移入回收站。", "ok"); onChanged(); }
    catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); setDeleting(null); }
  }

  if (entries.length === 0) return <div className="vault-empty">没有符合条件的条目。</div>;
  return (
    <div className="vault-rows" role="list">
      <div className="vault-row head" aria-hidden="true">
        <span /><span>标题</span><span>平台</span><span>类型</span><span>到期</span><span>更新于</span><span className="ops">操作</span>
      </div>
      {entries.map((entry) => {
        const field = mainField(entry);
        return (
          <div key={entry.id} role="listitem" className="vault-row" onClick={() => onView(entry)}>
            <i className="ti ti-key" aria-hidden="true" />
            <span className="title" translate="no" title={entry.title}>{entry.title}</span>
            <span className="mut" translate="no">{entry.platform || "—"}</span>
            <span className="mut">{KIND_LABELS[entry.kind]}</span>
            <ExpiryBadge expiresAt={entry.expiresAt} today={today} />
            <span className="mut">{formatTime(entry.updatedAt).slice(0, 10)}</span>
            <span className="ops" onClick={(event) => event.stopPropagation()}>
              <button className="gh xs" onClick={() => onView(entry)}><i className="ti ti-eye" /> 查看</button>
              <button className="gh xs" disabled={!field} title={field ? `复制 ${field}` : undefined} onClick={() => field && void copy(entry, field)}><i className="ti ti-copy" /> 复制</button>
              <button className="gh xs" onClick={() => onEdit(entry)}><i className="ti ti-edit" /> 编辑</button>
              <button className="gh xs" title="删除条目" aria-label="删除条目" onClick={() => setDeleting(entry)}><i className="ti ti-trash" /></button>
            </span>
          </div>
        );
      })}
      {deleting && (
        <ConfirmModal title="删除条目" danger message={`删除「${deleting.title}」？可在回收站保留 30 天。`} confirmLabel="删除条目" busy={busy}
          onClose={() => setDeleting(null)} onConfirm={() => void remove(deleting)} />
      )}
    </div>
  );
}
