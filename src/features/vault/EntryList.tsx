import { useState } from "react";
import { ConfirmModal, useToast } from "../../ui";
import { vaultApi, vaultError, type EntryView } from "./api";
import { KIND_LABELS } from "./labels";
import { accountOf, daysUntil, expiryState, formatTime, groupEntries, type EntryGroup } from "./vaultView";
import { useI18n } from "../../i18n";
import { aiBrief } from "./aiBrief";
import { publicKeyOf } from "./SshKeys";

/** How many sites the list draws before asking to show more. */
const STEP = 100;

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
  const [merging, setMerging] = useState<EntryGroup | null>(null);
  const [busy, setBusy] = useState(false);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState<Set<string>>(new Set());
  const [shown, setShown] = useState(STEP);
  // SSH keys go to ~/.ssh and never into Credential Manager, so they are not picked.
  const pickable = entries.filter((entry) => entry.kind !== "ssh_key");
  const chosen = pickable.filter((entry) => picked.has(entry.id));
  const allPicked = pickable.length > 0 && chosen.length === pickable.length;
  const groups = groupEntries(entries);
  const toggle = (ids: string[], on: boolean) => setPicked((old) => {
    const next = new Set(old);
    for (const id of ids) { if (on) next.add(id); else next.delete(id); }
    return next;
  });
  const flip = (key: string) => setOpen((old) => { const next = new Set(old); if (next.has(key)) next.delete(key); else next.add(key); return next; });

  async function setWindows(on: boolean) {
    setBusy(true);
    try {
      const count = await vaultApi.setWindows(chosen.map((entry) => entry.id), on);
      toast(tr(on ? "已把 {count} 条放进 Windows 凭据管理器。" : "已把 {count} 条移出 Windows 凭据管理器。").replace("{count}", String(count)), "ok");
      setPicked(new Set());
      onChanged();
    } catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }

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
      const credentials = ssh ? [] : await vaultApi.credentialTargets(entry.id).catch(() => []) ?? [];
      await navigator.clipboard.writeText(aiBrief(entry, local, tr, holders, credentials));
      const secrets = entry.fields.filter((field) => field.secret && field.filled);
      const reachable = (name: string) => credentials.some((item) => item.field === name) || holders.some((item) => item.field === name);
      if (ssh && !local?.path) toast("已复制给 AI 的信息。私钥还没放到本机 ~/.ssh，AI 暂时连不上：先在详情里点「放到本机 ~/.ssh」。", "info");
      else if (!ssh && secrets.some((field) => !reachable(field.name))) toast(entry.windows ? "已复制给 AI 的信息。有的值太长，放不进 Windows 凭据管理器，AI 拿不到那一项。" : "已复制给 AI 的信息。这一条没有放进 Windows 凭据管理器，AI 拿不到值：需要的话勾选它，点「放进 Windows 凭据」。", "info");
      else toast(ssh ? "已复制给 AI 的信息，不含保密内容。" : "已复制给 AI 的信息：告诉了它凭据名和取值命令，不含密钥值。", "ok");
    } catch (error) { toast(vaultError(error), "err"); }
  }
  async function remove(entry: EntryView) {
    setBusy(true);
    try { await vaultApi.remove(entry.id); toast("已移入回收站。", "ok"); onChanged(); }
    catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); setDeleting(null); }
  }
  // Each account kept more than once becomes its newest entry; the others' passwords go to its history.
  async function merge(group: EntryGroup) {
    setBusy(true);
    try {
      let count = 0;
      for (const set of group.duplicates) count += await vaultApi.merge(set[0].id, set.slice(1).map((entry) => entry.id));
      toast(tr("已合并 {count} 条重复登录，旧密码留在保留那条的历史里，原条目在回收站。").replace("{count}", String(count)), "ok");
      onChanged();
    } catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); setMerging(null); }
  }

  const row = (entry: EntryView, inGroup: boolean) => {
    const field = mainField(entry);
    const publicKey = entry.kind === "ssh_key" ? publicKeyOf(entry) : "";
    const account = accountOf(entry);
    return (
      <div key={entry.id} role="listitem" className={"vault-row" + (picked.has(entry.id) ? " picked" : "") + (inGroup ? " child" : "")} onClick={() => onView(entry)}>
        <span onClick={(event) => event.stopPropagation()}>
          {entry.kind === "ssh_key"
            ? <input type="checkbox" className="vault-pick" disabled aria-label="SSH 密钥不进 Windows 凭据" title="SSH 密钥放在 ~/.ssh 给 ssh 用，不进 Windows 凭据管理器" />
            : <input type="checkbox" className="vault-pick" aria-label="选择" checked={picked.has(entry.id)} onChange={(e) => toggle([entry.id], e.target.checked)} />}
        </span>
        <i className={"ti " + (inGroup ? "ti-user" : "ti-key")} aria-hidden="true" />
        <span className="title" translate="no" title={entry.title}>{inGroup && account ? account : entry.title}
          {entry.windows && <i className="ti ti-brand-windows vault-win" title="已放进 Windows 凭据管理器，本机的其他程序可以按名字取用" />}
        </span>
        <span className="mut" translate="no">{inGroup ? entry.title : entry.platform || "—"}</span>
        <span className="mut">{KIND_LABELS[entry.kind]}</span>
        <ExpiryBadge expiresAt={entry.expiresAt} today={today} />
        <span className="mut">{formatTime(entry.updatedAt).slice(0, 10)}</span>
        <span className="ops" onClick={(event) => event.stopPropagation()}>
          <button className="gh xs" onClick={() => onView(entry)}><i className="ti ti-eye" /> 查看</button>
          {publicKey
            ? <button className="gh xs" title="复制公钥：交给服务器的那一行" onClick={() => void copyPublic(entry)}><i className="ti ti-copy" /> 复制</button>
            : <button className="gh xs" disabled={!field} title={field ? `复制 ${field}` : undefined} onClick={() => field && void copy(entry, field)}><i className="ti ti-copy" /> 复制</button>}
          <button className="gh xs ai-btn" title="复制一段可以直接贴给 AI 的信息：怎么在这台电脑上用这一条（连接命令、Windows 凭据名和取值命令），不含私钥、口令、密钥值" onClick={() => void copyBrief(entry)}><i className="ti ti-sparkles" /> 给 AI</button>
          <button className="gh xs" onClick={() => onEdit(entry)}><i className="ti ti-edit" /> 编辑</button>
          <button className="gh xs" title="删除条目" aria-label="删除条目" onClick={() => setDeleting(entry)}><i className="ti ti-trash" /></button>
        </span>
      </div>
    );
  };

  const groupRow = (group: EntryGroup) => {
    const ids = group.entries.filter((entry) => entry.kind !== "ssh_key").map((entry) => entry.id);
    const all = ids.length > 0 && ids.every((id) => picked.has(id));
    const expanded = open.has(group.key);
    const extra = group.duplicates.reduce((sum, set) => sum + set.length - 1, 0);
    const newest = Math.max(...group.entries.map((entry) => entry.updatedAt));
    return <div key={`g:${group.key}`} className={"vault-row group" + (expanded ? " open" : "")} onClick={() => flip(group.key)}>
      <span onClick={(event) => event.stopPropagation()}>
        <input type="checkbox" className="vault-pick" aria-label="选择这个网站的全部条目" checked={all} disabled={!ids.length} onChange={(e) => toggle(ids, e.target.checked)} />
      </span>
      <i className={"ti " + (expanded ? "ti-chevron-down" : "ti-chevron-right")} aria-hidden="true" />
      <span className="title" translate="no" title={group.key}>{group.key}</span>
      <span className="mut">{tr("{count} 条").replace("{count}", String(group.entries.length))}</span>
      <span className="mut">{group.entries.some((entry) => entry.windows) ? <i className="ti ti-brand-windows vault-win" title="其中有条目放进了 Windows 凭据管理器" /> : ""}</span>
      <span />
      <span className="mut">{formatTime(newest).slice(0, 10)}</span>
      <span className="ops" onClick={(event) => event.stopPropagation()}>
        {extra > 0 && <button className="gh xs" title="同一网站同一账号存了几份（常见于从几个浏览器各导入一次）：合成一条，旧密码留在历史里" onClick={() => setMerging(group)}>
          <i className="ti ti-arrows-join" /> {tr("合并 {count} 条重复").replace("{count}", String(extra))}
        </button>}
        <button className="gh xs" onClick={() => flip(group.key)}><i className={"ti " + (expanded ? "ti-chevron-up" : "ti-list")} /> {expanded ? "收起" : "展开"}</button>
      </span>
    </div>;
  };

  if (entries.length === 0) return <div className="vault-empty">没有符合条件的条目。</div>;
  return (
    <div className="vault-rows" role="list">
      {chosen.length > 0 && <div className="vault-batch">
        <span>{tr("已选 {count} 条").replace("{count}", String(chosen.length))}</span>
        <button className="gh sm" disabled={busy} title="本机的其他程序（比如 AI）可以按名字从 Windows 凭据管理器取用这些值" onClick={() => void setWindows(true)}><i className="ti ti-brand-windows" /> 放进 Windows 凭据</button>
        <button className="gh sm" disabled={busy} onClick={() => void setWindows(false)}><i className="ti ti-lock" /> 移出，只留在保管库</button>
        <button className="gh sm" disabled={busy} onClick={() => setPicked(new Set())}>清空选择</button>
      </div>}
      <div className="vault-row head">
        <input type="checkbox" className="vault-pick" aria-label="全选" checked={allPicked} disabled={pickable.length === 0}
          onChange={() => setPicked(allPicked ? new Set() : new Set(pickable.map((entry) => entry.id)))} />
        <span />
        <span>标题</span><span>平台</span><span>类型</span><span>到期</span><span>更新于</span><span className="ops">操作</span>
      </div>
      {groups.slice(0, shown).map((group) => group.entries.length === 1
        ? row(group.entries[0], false)
        : [groupRow(group), ...(open.has(group.key) ? group.entries.map((entry) => row(entry, true)) : [])])}
      {groups.length > shown && <div className="vault-more">
        <button className="gh sm" onClick={() => setShown((n) => n + STEP)}>{tr("再显示 {count} 个网站").replace("{count}", String(Math.min(STEP, groups.length - shown)))}</button>
        <span className="mut">{tr("已显示 {shown} / {total} 个网站，可以用上面的搜索和筛选缩小范围。").replace("{shown}", String(shown)).replace("{total}", String(groups.length))}</span>
      </div>}
      {deleting && (
        <ConfirmModal title="删除条目" danger message={`删除「${deleting.title}」？可在回收站保留 30 天。`} confirmLabel="删除条目" busy={busy}
          onClose={() => setDeleting(null)} onConfirm={() => void remove(deleting)} />
      )}
      {merging && (
        <ConfirmModal title="合并重复登录" icon="ti-arrows-join" busy={busy} confirmLabel="合并"
          message={tr("「{site}」里同一账号存了几份：每个账号只保留最近更新的那条，其余各条的密码放进它的历史（详情里点「历史」能看到），原条目移进回收站，30 天内可以恢复。").replace("{site}", merging.key)}
          onClose={() => setMerging(null)} onConfirm={() => void merge(merging)} />
      )}
    </div>
  );
}
