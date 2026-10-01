import { useState } from "react";
import { Modal, useToast } from "../../ui";
import { vaultApi, vaultError, type EntryInput, type EntryView, type Kind } from "./api";
import { KIND_LABELS, KIND_ORDER, PLATFORM_PRESETS, TEMPLATES } from "./labels";

type DraftField = { key: number; name: string; previousName: string | null; value: string; secret: boolean; saved: boolean };
export type Draft = {
  id: string | null; title: string; platform: string; kind: Kind; fields: DraftField[];
  expiresAt: string; tags: string; note: string; favorite: boolean; touched: boolean;
};

let nextKey = 1;
const PRIVATE_KEY_CHARS = 16 * 1024;

function templateFields(kind: Kind): DraftField[] {
  return TEMPLATES[kind].map((field) => ({ key: nextKey++, name: field.name, previousName: null, value: "", secret: field.secret, saved: false }));
}

export function draftFrom(entry: EntryView | null): Draft {
  if (!entry) {
    return { id: null, title: "", platform: "", kind: "api_key", fields: templateFields("api_key"), expiresAt: "", tags: "", note: "", favorite: false, touched: false };
  }
  return {
    id: entry.id, title: entry.title, platform: entry.platform, kind: entry.kind,
    fields: entry.fields.map((field) => ({
      key: nextKey++, name: field.name, previousName: field.name, value: field.secret ? "" : field.value ?? "", secret: field.secret, saved: field.secret && field.filled,
    })),
    expiresAt: entry.expiresAt ?? "", tags: entry.tags.join(", "), note: entry.note, favorite: entry.favorite, touched: true,
  };
}

/** A saved secret left empty is sent as null, which keeps its value on the backend. */
export function toEntryInput(draft: Draft): EntryInput {
  const tags = [...new Set(draft.tags.split(/[,，]/).map((tag) => tag.trim()).filter(Boolean))];
  return {
    id: draft.id, title: draft.title.trim(), platform: draft.platform.trim(), kind: draft.kind,
    fields: draft.fields.filter((field) => field.name.trim()).map((field) => ({
      name: field.name.trim(),
      previousName: field.previousName,
      value: field.secret && field.saved && field.value === "" ? null : field.value,
      secret: field.secret,
    })),
    expiresAt: draft.expiresAt || null, tags, note: draft.note, favorite: draft.favorite,
  };
}

export function EntryEditor({ entry, onSaved, onClose }: { entry: EntryView | null; onSaved: (view: EntryView) => void; onClose: () => void }) {
  const toast = useToast();
  const [draft, setDraft] = useState<Draft>(() => draftFrom(entry));
  const [busy, setBusy] = useState(false);
  const update = (patch: Partial<Draft>) => setDraft((current) => ({ ...current, ...patch, touched: true }));
  const updateField = (key: number, patch: Partial<DraftField>) =>
    setDraft((current) => ({ ...current, touched: true, fields: current.fields.map((field) => field.key === key ? { ...field, ...patch } : field) }));

  function changeKind(kind: Kind) {
    // A new entry whose fields were never edited takes the new template.
    const untouched = draft.id === null && draft.fields.every((field) => field.value === "");
    setDraft((current) => ({ ...current, kind, fields: untouched ? templateFields(kind) : current.fields, touched: true }));
  }

  async function submit() {
    if (!draft.title.trim()) { toast("请填写标题。", "info"); return; }
    setBusy(true);
    try { onSaved(await vaultApi.save(toEntryInput(draft))); toast("已保存。", "ok"); }
    catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }

  return (
    <Modal wide title={entry ? "编辑条目" : "新建条目"} icon="ti-key" onClose={busy ? undefined : onClose}
      footer={<><button className="gh sm" disabled={busy} onClick={onClose}>取消</button><button className="pr sm" disabled={busy} onClick={() => void submit()}>保存</button></>}>
      <div className="vault-form">
        <label>标题<input className="ip full" value={draft.title} onChange={(e) => update({ title: e.target.value })} /></label>
        <label>平台
          <input className="ip full" list="vault-platforms" value={draft.platform} onChange={(e) => update({ platform: e.target.value })} />
          <datalist id="vault-platforms">{PLATFORM_PRESETS.map((name) => <option key={name} value={name} />)}</datalist>
        </label>
        <label>类型
          <div className="seg">{KIND_ORDER.map((kind) => <button key={kind} className={draft.kind === kind ? "on" : ""} onClick={() => changeKind(kind)}>{KIND_LABELS[kind]}</button>)}</div>
        </label>
        <div>
          {draft.fields.map((field) => {
            const multiline = draft.kind === "ssh_key" && field.secret && field.name.includes("私钥");
            const placeholder = field.saved ? "已保存，留空则保持不变" : "";
            return (
              <div className="vault-edit-field" key={field.key}>
                <input className="ip" value={field.name} aria-label="字段名" onChange={(e) => updateField(field.key, { name: e.target.value })} />
                {multiline
                  ? <textarea className="ip" value={field.value} maxLength={PRIVATE_KEY_CHARS} placeholder={placeholder} onChange={(e) => updateField(field.key, { value: e.target.value })} />
                  : <input className="ip" type={field.secret ? "password" : "text"} autoComplete="off" value={field.value} placeholder={placeholder} onChange={(e) => updateField(field.key, { value: e.target.value })} />}
                <label className="sw" title="保密"><input type="checkbox" checked={field.secret} onChange={(e) => updateField(field.key, { secret: e.target.checked })} /><span className="tk" /></label>
                <button className="gh sm" title="删除字段" onClick={() => setDraft((current) => ({ ...current, touched: true, fields: current.fields.filter((item) => item.key !== field.key) }))}><i className="ti ti-x" /></button>
              </div>
            );
          })}
          <button className="gh sm" onClick={() => setDraft((current) => ({ ...current, touched: true, fields: [...current.fields, { key: nextKey++, name: "", previousName: null, value: "", secret: true, saved: false }] }))}>
            <i className="ti ti-plus" /> 添加字段
          </button>
        </div>
        <label>到期日<input className="ip" type="date" value={draft.expiresAt} onChange={(e) => update({ expiresAt: e.target.value })} /></label>
        <label>标签（用逗号分隔）<input className="ip full" value={draft.tags} onChange={(e) => update({ tags: e.target.value })} /></label>
        <label>备注<textarea className="ip full" rows={3} value={draft.note} placeholder="例如：怎么生成的、绑定哪个邮箱、权限范围" onChange={(e) => update({ note: e.target.value })} /></label>
        <label style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
          <input type="checkbox" checked={draft.favorite} onChange={(e) => update({ favorite: e.target.checked })} /> 收藏并置顶
        </label>
      </div>
    </Modal>
  );
}
