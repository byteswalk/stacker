import { useEffect, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../i18n";
import { ConfirmModal, useToast } from "../../ui";
import { vaultApi, vaultError, type EntryView } from "./api";
import { KIND_LABELS, RISK_LABELS } from "./labels";
import { ExpiryBadge } from "./EntryList";
import { HistoryDialog } from "./HistoryDialog";
import { useRevealed } from "./useRevealed";

export function EntryDetail({ entry, today, onEdit, onChanged }: {
  entry: EntryView; today: Date; onEdit: () => void; onChanged: () => void;
}) {
  const toast = useToast();
  const { tr } = useI18n();
  const { revealed, show, hide, clear } = useRevealed();
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [exportPath, setExportPath] = useState<string | null>(null);
  const currentId = useRef(entry.id);

  // Revealed values belong to one saved version of one entry.
  useEffect(() => { currentId.current = entry.id; clear(); }, [entry.id, entry.updatedAt, clear]);

  async function run(action: () => Promise<unknown>, done?: string) {
    setBusy(true);
    try { await action(); if (done) toast(done, "ok"); onChanged(); }
    catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }
  async function reveal(field: string) {
    if (revealed[field] !== undefined) { hide(field); return; }
    const id = entry.id;
    try {
      const value = await vaultApi.reveal(id, field);
      if (currentId.current === id) show(field, value);
    } catch (error) { toast(vaultError(error), "err"); }
  }
  async function copy(field: string) {
    try { await vaultApi.copy(entry.id, field); toast("已复制，30 秒后自动清除剪贴板。", "ok"); } catch (error) { toast(vaultError(error), "err"); }
  }
  async function exportKey() {
    try {
      const dest = await save({ title: tr("导出私钥"), defaultPath: entry.title.replace(/[\\/:*?"<>|]/g, "_") });
      if (dest) setExportPath(dest);
    } catch (error) { toast(vaultError(error), "err"); }
  }

  return (
    <div className="pxcard">
      <div className="pxsec"><i className={"ti " + (entry.favorite ? "ti-star-filled" : "ti-key")} /> {entry.title}
        <span className="pxhint">{entry.platform ? `${entry.platform} · ` : ""}{KIND_LABELS[entry.kind]}</span>
      </div>
      {entry.fields.map((field) => {
        const shown = field.secret ? revealed[field.name] : field.value ?? "";
        return (
          <div className="vault-field" key={field.name}>
            <span className="name">{field.name}</span>
            <code>{!field.filled ? "—" : shown !== undefined ? shown : "••••••••"}</code>
            <span style={{ display: "flex", gap: 4 }}>
              {field.secret && field.filled && (
                <button className="gh sm" title={revealed[field.name] !== undefined ? "隐藏" : "显示"} aria-label={revealed[field.name] !== undefined ? "隐藏" : "显示"} onClick={() => void reveal(field.name)}>
                  <i className={"ti " + (revealed[field.name] !== undefined ? "ti-eye-off" : "ti-eye")} />
                </button>
              )}
              {field.filled && <button className="gh sm" title="复制" aria-label="复制" onClick={() => void copy(field.name)}><i className="ti ti-copy" /></button>}
            </span>
          </div>
        );
      })}
      {entry.ssh && (
        <div className="vault-field">
          <span className="name">密钥信息</span>
          <code>{entry.ssh.algorithm}{entry.ssh.bits ? ` · ${entry.ssh.bits} 位` : ""}{entry.ssh.fingerprint ? `\n${entry.ssh.fingerprint}` : ""}</code>
          <span className="vault-tags">{entry.ssh.risks.map((risk) => <span key={risk} className="vault-badge expired">{RISK_LABELS[risk] ?? risk}</span>)}</span>
        </div>
      )}
      <div className="vault-field"><span className="name">到期</span><span><ExpiryBadge expiresAt={entry.expiresAt} today={today} /></span><span /></div>
      {entry.tags.length > 0 && <div className="vault-field"><span className="name">标签</span><span className="vault-tags">{entry.tags.map((tag) => <span key={tag} className="vault-badge">{tag}</span>)}</span><span /></div>}
      {entry.note && <div className="vault-field"><span className="name">备注</span><code>{entry.note}</code><span /></div>}
      <div className="vault-actions">
        <button className="gh sm" disabled={busy} onClick={() => void run(() => vaultApi.favorite(entry.id, !entry.favorite))}>
          <i className={"ti " + (entry.favorite ? "ti-star-off" : "ti-star")} /> {entry.favorite ? "取消收藏" : "收藏"}
        </button>
        {entry.historyCount > 0 && <button className="gh sm" onClick={() => setHistoryOpen(true)}><i className="ti ti-history" /> 历史</button>}
        {entry.kind === "ssh_key" && entry.ssh && <button className="gh sm" disabled={busy} onClick={() => void exportKey()}><i className="ti ti-file-export" /> 导出私钥</button>}
        <button className="gh sm" disabled={busy} onClick={() => setConfirmDelete(true)}><i className="ti ti-trash" /> 删除</button>
        <button className="pr sm" disabled={busy} onClick={onEdit}><i className="ti ti-edit" /> 编辑</button>
      </div>
      {confirmDelete && (
        <ConfirmModal title="删除条目" danger message={`删除「${entry.title}」？可在回收站保留 30 天。`} confirmLabel="删除" busy={busy}
          onClose={() => setConfirmDelete(false)}
          onConfirm={() => { setConfirmDelete(false); void run(() => vaultApi.remove(entry.id), "已移入回收站。"); }} />
      )}
      {exportPath !== null && (
        <ConfirmModal title="导出私钥" danger message="私钥将以明文写入所选位置，请确认路径安全。" confirmLabel="导出" busy={busy}
          onClose={() => setExportPath(null)}
          onConfirm={() => { const dest = exportPath; setExportPath(null); void run(() => vaultApi.sshExport(entry.id, dest), "私钥已导出，文件权限已收紧为仅当前用户。"); }} />
      )}
      {historyOpen && <HistoryDialog entryId={entry.id} onClose={() => setHistoryOpen(false)} />}
    </div>
  );
}
