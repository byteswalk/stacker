import { useEffect, useState } from "react";
import { Modal, useToast } from "../../ui";
import { vaultApi, vaultError, type HistoryView } from "./api";
import { useRevealed } from "./useRevealed";
import { formatTime } from "./vaultView";

export function HistoryDialog({ entryId, onClose }: { entryId: string; onClose: () => void }) {
  const toast = useToast();
  const [items, setItems] = useState<HistoryView[] | null>(null);
  const { revealed, show, hide } = useRevealed();

  useEffect(() => {
    let ignore = false;
    vaultApi.history(entryId).then((list) => { if (!ignore) setItems(list); }).catch((error) => { if (!ignore) { toast(vaultError(error), "err"); setItems([]); } });
    return () => { ignore = true; };
  }, [entryId, toast]);

  async function reveal(index: number) {
    const key = String(index);
    if (revealed[key] !== undefined) { hide(key); return; }
    try { show(key, await vaultApi.historyReveal(entryId, index)); } catch (error) { toast(vaultError(error), "err"); }
  }
  async function copy(index: number) {
    try { await vaultApi.historyCopy(entryId, index); toast("已复制，30 秒后自动清除剪贴板。", "ok"); } catch (error) { toast(vaultError(error), "err"); }
  }

  return (
    <Modal title="历史版本" icon="ti-history" sub="保密字段被修改前的值，每个字段保留最近 10 次。" onClose={onClose}>
      {items === null ? <div className="vault-empty">正在读取…</div> : items.length === 0 ? <div className="vault-empty">暂无历史版本。</div> : items.map((item) => (
        <div className="vault-field" key={item.index}>
          <span className="name">{item.field}<br />{formatTime(item.at)}</span>
          <code translate="no">{revealed[String(item.index)] ?? "••••••••"}</code>
          <span style={{ display: "flex", gap: 4 }}>
            <button className="gh sm" title="显示内容" aria-label="显示内容" onClick={() => void reveal(item.index)}><i className="ti ti-eye" /></button>
            <button className="gh sm" title="复制" aria-label="复制" onClick={() => void copy(item.index)}><i className="ti ti-copy" /></button>
          </span>
        </div>
      ))}
    </Modal>
  );
}
