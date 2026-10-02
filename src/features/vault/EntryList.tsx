import { useState } from "react";
import { ConfirmModal, useToast } from "../../ui";
import { vaultApi, vaultError, type EntryView } from "./api";
import { KIND_LABELS } from "./labels";
import { daysUntil, expiryState, formatTime } from "./vaultView";
import { useI18n } from "../../i18n";
import { aiBrief } from "./aiBrief";
import { publicKeyOf } from "./SshKeys";

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
  const { tr } = useI18n();
  const [deleting, setDeleting] = useState<EntryView | null>(null);
  const [busy, setBusy] = useState(false);

  async function copy(entry: EntryView, field: string) {
    try { await vaultApi.copy(entry.id, field); toast("已复制，30 秒后自动清除剪贴板。", "ok"); } catch (error) { toast(vaultError(error), "err"); }
  }
  // What gets handed around for an SSH key is its public key, which is no secret.
  async function copyPublic(entry: EntryView) {
    try { await navigator.clipboard.writeText(publicKeyOf(entry)); toast("已复制公钥。", "ok"); }
    catch { toast("复制失败，请手动选中复制。", "err"); }
  }
  // The text for an AI agent: how to use the entry, without its secrets.
  async function copyBrief(entry: EntryView) {
    try {
      const ssh = entry.kind === "ssh_key";
      const local = ssh ? await vaultApi.sshLocal(entry.id).catch(() => null) : null;
      const holders = ssh ? [] : await vaultApi.envHolders(entry.id).catch(() => []) ?? [];
      await navigator.clipboard.writeText(aiBrief(entry, local, tr, holders));
      const secrets = entry.fields.filter((field) => field.secret && field.filled);
      if (ssh && !local?.path) toast("已复制给 AI 的信息。私钥还没放到本机 ~/.ssh，AI 暂时连不上：先在详情里点「放到本机 ~/.ssh」。", "info");
      else if (!ssh && secrets.some((field) => !holders.some((item) => item.field === field.name))) toast("已复制给 AI 的信息。密钥值只在保管库里、本机没有环境变量保存它，所以 AI 拿不到值。", "info");
      else toast(ssh ? "已复制给 AI 的信息，不含保密内容。" : "已复制给 AI 的信息：告诉了它用哪个环境变量，不含密钥值。", "ok");
    } catch (error) { toast(vaultError(error), "err"); }
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
        const publicKey = entry.kind === "ssh_key" ? publicKeyOf(entry) : "";
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
              {publicKey
                ? <button className="gh xs" title="复制公钥：交给服务器的那一行" onClick={() => void copyPublic(entry)}><i className="ti ti-copy" /> 复制</button>
                : <button className="gh xs" disabled={!field} title={field ? `复制 ${field}` : undefined} onClick={() => field && void copy(entry, field)}><i className="ti ti-copy" /> 复制</button>}
              <button className="gh xs ai-btn" title="复制一段可以直接贴给 AI 的信息：怎么在这台电脑上用这一条（连接命令、环境变量名），不含私钥、口令、密钥值" onClick={() => void copyBrief(entry)}><i className="ti ti-sparkles" /> 给 AI</button>
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
