import { useCallback, useEffect, useState } from "react";
import { ConfirmModal, Modal, useToast } from "../../ui";
import { vaultApi, vaultError, type EntryView } from "./api";
import { formatTime } from "./vaultView";

export function TrashDialog({ onClose, onChanged }: { onClose: () => void; onChanged: () => void }) {
  const toast = useToast();
  const [items, setItems] = useState<EntryView[] | null>(null);
  const [purging, setPurging] = useState<EntryView | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => vaultApi.list(true).then(setItems).catch((error) => toast(vaultError(error), "err")), [toast]);
  useEffect(() => { void load(); }, [load]);

  async function run(action: () => Promise<void>, done: string) {
    setBusy(true);
    try { await action(); toast(done, "ok"); await load(); onChanged(); }
    catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }

  return (
    <Modal title="回收站" icon="ti-trash" sub="删除的条目保留 30 天，到期后在下次解锁时清除。" onClose={busy ? undefined : onClose}>
      {items === null ? <div className="vault-empty">正在读取…</div> : items.length === 0 ? <div className="vault-empty">回收站是空的。</div> : items.map((item) => (
        <div className="vault-field" key={item.id}>
          <span className="name">{item.title}</span>
          <span className="mut">删除于 {formatTime(item.deletedAt ?? 0)}</span>
          <span style={{ display: "flex", gap: 4 }}>
            <button className="gh sm" disabled={busy} onClick={() => void run(() => vaultApi.restore(item.id), "已恢复。")}>恢复</button>
            <button className="gh sm" disabled={busy} onClick={() => setPurging(item)}>立即清除</button>
          </span>
        </div>
      ))}
      {purging && (
        <ConfirmModal title="立即清除" danger message={`立即清除「${purging.title}」？此操作无法撤销。`} confirmLabel="立即清除" busy={busy}
          onClose={() => setPurging(null)}
          onConfirm={() => { const target = purging; setPurging(null); void run(() => vaultApi.purge(target.id), "已清除。"); }} />
      )}
    </Modal>
  );
}
