import { useEffect, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../i18n";
import { ConfirmModal, Modal, useToast } from "../../ui";
import { vaultApi, vaultError, type CredentialTarget, type EntryView } from "./api";
import { KIND_LABELS, RISK_LABELS } from "./labels";
import { ExpiryBadge } from "./EntryList";
import { HistoryDialog } from "./HistoryDialog";
import { useRevealed } from "./useRevealed";
import { publicKeyOf, SERVERS_FIELD, SshKeyActions, SshServers } from "./SshKeys";

/** One entry in a dialog: its fields to show and copy, and what can be done with it. */
export function EntryDetail({ entry, today, onEdit, onChanged, onClose }: {
  entry: EntryView; today: Date; onEdit: () => void; onChanged: () => void; onClose: () => void;
}) {
  const toast = useToast();
  const { tr } = useI18n();
  const { revealed, show, hide, clear } = useRevealed();
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [exportPath, setExportPath] = useState<string | null>(null);
  const currentId = useRef(entry.id);
  const [credentials, setCredentials] = useState<CredentialTarget[]>([]);
  // Under which names Windows Credential Manager holds this entry's secrets.
  useEffect(() => {
    let alive = true;
    vaultApi.credentialTargets(entry.id).then((list) => { if (alive) setCredentials(list ?? []); }).catch(() => { if (alive) setCredentials([]); });
    return () => { alive = false; };
  }, [entry.id, entry.updatedAt]);

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
    <Modal wide icon="ti-key" title={<span translate="no">{entry.title}</span>} onClose={onClose}
      sub={<span>{entry.platform ? <span translate="no">{entry.platform} · </span> : null}{KIND_LABELS[entry.kind]}</span>}
      footer={<>
        {entry.historyCount > 0 && <button className="gh sm" onClick={() => setHistoryOpen(true)}><i className="ti ti-history" /> 历史</button>}
        {entry.kind === "ssh_key" && entry.ssh && <button className="gh sm" disabled={busy} onClick={() => void exportKey()}><i className="ti ti-file-export" /> 导出私钥</button>}
        <button className="gh sm" disabled={busy} onClick={() => setConfirmDelete(true)}><i className="ti ti-trash" /> 删除条目</button>
        <button className="pr sm" disabled={busy} onClick={onEdit}><i className="ti ti-edit" /> 编辑</button>
      </>}>
      <div className="vault-detail">
      {entry.fields.filter((field) => !(entry.kind === "ssh_key" && field.name === SERVERS_FIELD)).map((field) => {
        const shown = field.secret ? revealed[field.name] : field.value ?? "";
        return (
          <div className="vault-field" key={field.name}>
            <span className="name">{field.name}</span>
            <code translate="no">{!field.filled ? "—" : shown !== undefined ? shown : "••••••••"}</code>
            <span style={{ display: "flex", gap: 4 }}>
              {field.secret && field.filled && (
                <button className="gh sm" title={revealed[field.name] !== undefined ? "隐藏内容" : "显示内容"} aria-label={revealed[field.name] !== undefined ? "隐藏内容" : "显示内容"} onClick={() => void reveal(field.name)}>
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
      {entry.kind === "ssh_key" && <>
        {publicKeyOf(entry) !== "" && <div className="vault-field"><span className="name">交给服务器</span><SshKeyActions entry={entry} onChanged={onChanged} /><span /></div>}
        <SshServers entry={entry} onChanged={onChanged} />
      </>}
      {credentials.length > 0 && <div className="vault-field" title="保密值同步了一份到 Windows 凭据管理器，本机的其他程序（比如 AI）可以按这个名字取用；这里显示的值也是从那里读出来的">
        <span className="name">Windows 凭据</span>
        <code translate="no">{credentials.map((item) => item.target).join("\n")}</code>
        <span />
      </div>}
      <div className="vault-field"><span className="name">到期</span><span><ExpiryBadge expiresAt={entry.expiresAt} today={today} /></span><span /></div>
      {entry.tags.length > 0 && <div className="vault-field"><span className="name">标签</span><span className="vault-tags">{entry.tags.map((tag) => <span key={tag} className="vault-badge">{tag}</span>)}</span><span /></div>}
      {entry.note && <div className="vault-field"><span className="name">备注</span><code translate="no">{entry.note}</code><span /></div>}
      </div>
      {confirmDelete && (
        <ConfirmModal title="删除条目" danger message={`删除「${entry.title}」？可在回收站保留 30 天。`} confirmLabel="删除条目" busy={busy}
          onClose={() => setConfirmDelete(false)}
          onConfirm={() => { setConfirmDelete(false); void run(() => vaultApi.remove(entry.id), "已移入回收站。").then(onClose); }} />
      )}
      {exportPath !== null && (
        <ConfirmModal title="导出私钥" danger message="私钥将以明文写入所选位置，请确认路径安全。" confirmLabel="导出" busy={busy}
          onClose={() => setExportPath(null)}
          onConfirm={() => { const dest = exportPath; setExportPath(null); void run(() => vaultApi.sshExport(entry.id, dest), "私钥已导出，文件权限已收紧为仅当前用户。"); }} />
      )}
      {historyOpen && <HistoryDialog entryId={entry.id} onClose={() => setHistoryOpen(false)} />}
    </Modal>
  );
}
