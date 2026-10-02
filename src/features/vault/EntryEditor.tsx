import { useRef, useState } from "react";
import { Modal, useToast } from "../../ui";
import { vaultApi, vaultError, type EntryInput, type EntryView, type Kind } from "./api";
import { generalKind, KIND_LABELS, KIND_ORDER, TEMPLATES } from "./labels";

type DraftField = {
  key: number; name: string; previousName: string | null; value: string; secret: boolean; saved: boolean;
  /** The saved value once it has been read into the box; null while it has not. */
  original: string | null;
  shown: boolean;
};
export type Draft = {
  id: string | null; title: string; platform: string; kind: Kind; fields: DraftField[];
  expiresAt: string; tags: string; note: string; favorite: boolean; touched: boolean;
};

let nextKey = 1;
const MAX_VALUE_BYTES = 16 * 1024;

function newField(name: string, secret: boolean): DraftField {
  return { key: nextKey++, name, previousName: null, value: "", secret, saved: false, original: null, shown: false };
}

function templateFields(kind: Kind): DraftField[] {
  return TEMPLATES[kind].map((field) => newField(field.name, field.secret));
}

export function draftFrom(entry: EntryView | null): Draft {
  if (!entry) {
    return { id: null, title: "", platform: "", kind: "other", fields: templateFields("other"), expiresAt: "", tags: "", note: "", favorite: false, touched: false };
  }
  return {
    id: entry.id, title: entry.title, platform: entry.platform, kind: generalKind(entry.kind),
    fields: entry.fields.map((field) => ({
      ...newField(field.name, field.secret), previousName: field.name,
      value: field.secret ? "" : field.value ?? "", saved: field.secret && field.filled,
    })),
    expiresAt: entry.expiresAt ?? "", tags: entry.tags.join(", "), note: entry.note, favorite: entry.favorite, touched: true,
  };
}

/** A saved secret is sent as null, which keeps its value on the backend, while its box is left as it was: empty, or holding the value read into it. */
export function toEntryInput(draft: Draft): EntryInput {
  const tags = [...new Set(draft.tags.split(/[,，]/).map((tag) => tag.trim()).filter(Boolean))];
  return {
    id: draft.id, title: draft.title.trim(), platform: draft.platform.trim(), kind: draft.kind,
    fields: draft.fields.filter((field) => field.name.trim()).map((field) => ({
      name: field.name.trim(),
      previousName: field.previousName,
      value: field.saved && field.value === (field.original ?? "") ? null : field.value,
      secret: field.secret,
    })),
    expiresAt: draft.expiresAt || null, tags, note: draft.note, favorite: draft.favorite,
  };
}

/**
 * An expiry date as `yyyy-MM-dd`, from what was typed: `2026-11-02`, `2026/11/2`,
 * `2026.11.02` or `20261102`. Empty stays empty; anything that is not a real date is null.
 */
export function normalizeDate(text: string): string | null {
  const value = text.trim();
  if (!value) return "";
  const match = /^(\d{4})[-/.年]?(\d{1,2})[-/.月]?(\d{1,2})日?$/.exec(value);
  if (!match) return null;
  const [year, month, day] = match.slice(1).map(Number);
  const date = new Date(year, month - 1, day);
  if (date.getFullYear() !== year || date.getMonth() !== month - 1 || date.getDate() !== day) return null;
  return `${year}-${String(month).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
}

/** Name of the first field whose value is larger than 16 KB (the backend limit), or null. */
export function oversizedField(draft: Draft): string | null {
  const encoder = new TextEncoder();
  return draft.fields.find((field) => encoder.encode(field.value).length > MAX_VALUE_BYTES)?.name ?? null;
}

export function EntryEditor({ entry, onSaved, onClose }: { entry: EntryView | null; onSaved: (view: EntryView) => void; onClose: () => void }) {
  const toast = useToast();
  const [draft, setDraft] = useState<Draft>(() => draftFrom(entry));
  const [busy, setBusy] = useState(false);
  const picker = useRef<HTMLInputElement>(null);
  const update = (patch: Partial<Draft>) => setDraft((current) => ({ ...current, ...patch, touched: true }));
  const updateField = (key: number, patch: Partial<DraftField>) =>
    setDraft((current) => ({ ...current, touched: true, fields: current.fields.map((field) => field.key === key ? { ...field, ...patch } : field) }));

  function changeKind(kind: Kind) {
    // A new entry whose fields were never edited takes the new template.
    const untouched = draft.id === null && draft.fields.every((field) => field.value === "");
    setDraft((current) => ({ ...current, kind, fields: untouched ? templateFields(kind) : current.fields, touched: true }));
  }

  // The eye on a saved secret reads the value into the box first, so what is being edited can be seen.
  async function toggleShown(field: DraftField) {
    if (field.shown) { updateField(field.key, { shown: false }); return; }
    if (!(field.saved && field.original === null && field.value === "" && draft.id && field.previousName)) {
      updateField(field.key, { shown: true });
      return;
    }
    try {
      const value = await vaultApi.reveal(draft.id, field.previousName);
      updateField(field.key, { value, original: value, shown: true });
    } catch (error) { toast(vaultError(error), "err"); }
  }

  // A saved value nobody has read is copied by the backend, the way the details copy it;
  // what is in the box (typed, pasted, or read in) is copied as it is.
  async function copyField(field: DraftField) {
    try {
      if (field.saved && field.original === null && field.value === "" && draft.id && field.previousName) {
        await vaultApi.copy(draft.id, field.previousName);
        toast("已复制，30 秒后自动清除剪贴板。", "ok");
      } else {
        await navigator.clipboard.writeText(field.value);
        toast("已复制。", "ok");
      }
    } catch (error) { toast(vaultError(error), "err"); }
  }

  async function submit() {
    if (!draft.title.trim()) { toast("请填写标题。", "info"); return; }
    const expiresAt = normalizeDate(draft.expiresAt);
    if (expiresAt === null) { toast("到期日请按 yyyy-MM-dd 填写，例如 2026-11-02。", "info"); return; }
    if (oversizedField(draft) !== null) { toast("字段内容超过 16 KB，请确认粘贴的内容。", "info"); return; }
    setBusy(true);
    try { onSaved(await vaultApi.save(toEntryInput({ ...draft, expiresAt }))); toast("已保存。", "ok"); }
    catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }

  return (
    <Modal wide title={entry ? "编辑条目" : "新建条目"} icon="ti-key" onClose={busy ? undefined : onClose}
      footer={<><button className="gh sm" disabled={busy} onClick={onClose}>取消</button><button className="pr sm" disabled={busy} onClick={() => void submit()}>保存</button></>}>
      <div className="vault-form vault-editor">
        <div className="vault-editor-pair">
          <label>标题<input className="ip full" autoFocus={!entry} value={draft.title} placeholder="例如：GitHub 个人令牌" onChange={(e) => update({ title: e.target.value })} /></label>
          <label>平台<input className="ip full" value={draft.platform} placeholder="选填，例如 GitHub" onChange={(e) => update({ platform: e.target.value })} /></label>
        </div>
        <div className="vault-editor-block">
          <span className="vault-editor-label">类型</span>
          <div className="seg">{KIND_ORDER.map((kind) => <button key={kind} type="button" className={draft.kind === kind ? "on" : ""} onClick={() => changeKind(kind)}>{KIND_LABELS[kind]}</button>)}</div>
        </div>
        <div className="vault-editor-block">
          <span className="vault-editor-label">凭据内容</span>
          <div className="vault-editor-fields">
            {draft.fields.length === 0 && <div className="vault-editor-none">还没有字段，点下面的「添加字段」。</div>}
            {draft.fields.map((field) => {
              // A private key keeps its line breaks, so it sits in a text area kept to one line.
              const multiline = draft.kind === "ssh_key" && field.secret && field.name.includes("私钥");
              const unread = field.saved && field.original === null && field.value === "";
              const placeholder = unread ? "已保存，留空则保持不变" : multiline ? "粘贴私钥（Ctrl+V）" : "";
              const eye = field.shown ? "隐藏内容" : "显示内容";
              return (
                <div className="vault-edit-field" key={field.key}>
                  <input className="ip" value={field.name} aria-label="字段名" placeholder="字段名" onChange={(e) => updateField(field.key, { name: e.target.value })} />
                  <div className="vault-edit-value">
                    {multiline
                      ? <textarea className={"ip oneline" + (field.secret && !field.shown ? " masked" : "")} rows={1} wrap="off" spellCheck={false} value={field.value} placeholder={placeholder}
                        title={field.value ? `${field.value.split("\n").filter(Boolean).length} 行` : undefined} onChange={(e) => updateField(field.key, { value: e.target.value })} />
                      : <input className="ip" type={field.secret && !field.shown ? "password" : "text"} autoComplete="off" spellCheck={false} value={field.value} placeholder={placeholder} onChange={(e) => updateField(field.key, { value: e.target.value })} />}
                    {field.secret && <button type="button" className="gh sm" title={eye} aria-label={eye} onClick={() => void toggleShown(field)}>
                      <i className={"ti " + (field.shown ? "ti-eye-off" : "ti-eye")} />
                    </button>}
                    <button type="button" className="gh sm" title="复制" aria-label="复制" disabled={!unread && field.value === ""} onClick={() => void copyField(field)}>
                      <i className="ti ti-copy" />
                    </button>
                  </div>
                  <label className="vault-edit-secret" title="保密的字段在列表和详情里默认用圆点遮住">
                    <span className="sw sm2"><input type="checkbox" aria-label="保密" checked={field.secret} onChange={(e) => updateField(field.key, { secret: e.target.checked })} /><span className="tk" /></span>
                    <span>保密</span>
                  </label>
                  <button type="button" className="gh sm" title="删除字段" aria-label="删除字段" onClick={() => setDraft((current) => ({ ...current, touched: true, fields: current.fields.filter((item) => item.key !== field.key) }))}><i className="ti ti-trash" /></button>
                </div>
              );
            })}
            <button type="button" className="gh sm vault-editor-add" onClick={() => setDraft((current) => ({ ...current, touched: true, fields: [...current.fields, newField("", true)] }))}>
              <i className="ti ti-plus" /> 添加字段
            </button>
          </div>
        </div>
        <div className="vault-editor-pair narrow">
          <label>到期日
            <span className="vault-date">
              <input className="ip full" value={draft.expiresAt} placeholder="yyyy-MM-dd" maxLength={10} inputMode="numeric" spellCheck={false}
                onChange={(e) => update({ expiresAt: e.target.value })}
                onBlur={() => { const fixed = normalizeDate(draft.expiresAt); if (fixed) update({ expiresAt: fixed }); }} />
              <button type="button" className="gh sm" title="选择日期" aria-label="选择日期" onClick={() => picker.current?.showPicker()}><i className="ti ti-calendar" /></button>
              <input ref={picker} type="date" className="vault-date-picker" tabIndex={-1} aria-hidden="true"
                value={normalizeDate(draft.expiresAt) ?? ""} onChange={(e) => update({ expiresAt: e.target.value })} />
            </span>
          </label>
          <label>标签（用逗号分隔）<input className="ip full" value={draft.tags} onChange={(e) => update({ tags: e.target.value })} /></label>
        </div>
        <label>备注<textarea className="ip full" rows={3} value={draft.note} placeholder="例如：怎么生成的、绑定哪个邮箱、权限范围" onChange={(e) => update({ note: e.target.value })} /></label>
      </div>
    </Modal>
  );
}
