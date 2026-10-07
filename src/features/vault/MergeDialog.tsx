import { useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { Modal, useToast } from "../../ui";
import { vaultApi, vaultError, type EntryView } from "./api";
import { accountOf, formatTime, type EntryGroup } from "./vaultView";

const urlOf = (entry: EntryView) => entry.fields.find((field) => field.name === "网址")?.value ?? "";

/**
 * What merging a site's duplicates would do, before it is done: per account, which entry is
 * kept (the newest unless another is picked) and which go to the trash, and whether a
 * password differs and so goes into the kept entry's history.
 */
export function MergeDialog({ group, onClose, onDone }: { group: EntryGroup; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const { tr } = useI18n();
  const [keep, setKeep] = useState<string[]>(() => group.duplicates.map((set) => set[0].id));
  const [left, setLeft] = useState<Set<string>>(new Set());
  const [differ, setDiffer] = useState<Record<string, boolean>>({});
  const [busy, setBusy] = useState(false);

  // Whether each other entry's password differs from the one kept, read again when the kept one changes.
  useEffect(() => {
    let alive = true;
    void Promise.all(group.duplicates.map(async (set, index) => {
      const others = set.filter((entry) => entry.id !== keep[index]).map((entry) => entry.id);
      const result = await vaultApi.mergePreview(keep[index], others).catch(() => others.map(() => true));
      return others.map((id, at) => [id, result[at]] as const);
    })).then((pairs) => { if (alive) setDiffer(Object.fromEntries(pairs.flat())); });
    return () => { alive = false; };
  }, [group, keep]);

  const merged = group.duplicates.reduce((sum, set, index) => sum + set.filter((entry) => entry.id !== keep[index] && !left.has(entry.id)).length, 0);

  async function merge() {
    setBusy(true);
    try {
      let count = 0;
      for (const [index, set] of group.duplicates.entries()) {
        const others = set.filter((entry) => entry.id !== keep[index] && !left.has(entry.id)).map((entry) => entry.id);
        if (others.length) count += await vaultApi.merge(keep[index], others);
      }
      toast(tr("已合并 {count} 条重复登录，旧密码留在保留那条的历史里，原条目在回收站。").replace("{count}", String(count)), "ok");
      onDone();
    } catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }

  return <Modal wide icon="ti-arrows-join" title="合并重复登录" sub={<span translate="no">{group.key}</span>} onClose={busy ? undefined : onClose}
    footer={<>
      <button className="gh sm" disabled={busy} onClick={onClose}>取消</button>
      <button className="pr sm" disabled={busy || merged === 0} onClick={() => void merge()}>
        <i className={"ti " + (busy ? "ti-loader spin" : "ti-arrows-join")} /> {tr("合并 {count} 条").replace("{count}", String(merged))}
      </button>
    </>}>
    <div className="vault-sub" style={{ margin: "0 0 10px" }}>每个账号保留选中的那条（默认最近更新的）；其余勾选的移进回收站，30 天内可以恢复。密码不同的，旧密码放进保留那条的「历史」，在条目详情底部能看到。</div>
    <div className="vault-merge">
      {group.duplicates.map((set, index) => <div className="vault-merge-set" key={set[0].id}>
        <div className="vault-merge-head"><i className="ti ti-user" /> <b translate="no">{accountOf(set[0])}</b> <span className="mut">{tr("{count} 份").replace("{count}", String(set.length))}</span></div>
        {set.map((entry) => {
          const kept = entry.id === keep[index];
          const leaving = !kept && !left.has(entry.id);
          const url = urlOf(entry);
          return <div className={"vault-merge-row" + (kept ? " kept" : leaving ? "" : " skipped")} key={entry.id}>
            <label title="保留这一条">
              <input type="radio" name={`keep-${index}`} checked={kept} disabled={busy}
                onChange={() => { setKeep((old) => old.map((id, at) => at === index ? entry.id : id)); setLeft((old) => { const next = new Set(old); next.delete(entry.id); return next; }); }} />
            </label>
            <div className="mt">
              <div className="mono" translate="no" title={url}>{url || entry.title}</div>
              <div className="mut">{tr("更新于 {date}").replace("{date}", formatTime(entry.updatedAt).slice(0, 16))}{entry.note ? <span translate="no"> · {entry.note}</span> : null}</div>
            </div>
            <span className={"vault-badge" + (kept ? " keep" : "")}>
              {kept ? "保留" : !leaving ? "不动" : differ[entry.id] === undefined ? "…" : differ[entry.id] ? "密码不同，旧密码进历史" : "密码相同"}
            </span>
            <input type="checkbox" className="vault-pick" aria-label="合并这一条" title={kept ? "保留的那条" : "勾选的合并进保留那条"} disabled={kept || busy} checked={leaving}
              onChange={(event) => setLeft((old) => { const next = new Set(old); if (event.target.checked) next.delete(entry.id); else next.add(entry.id); return next; })} />
          </div>;
        })}
      </div>)}
    </div>
  </Modal>;
}
