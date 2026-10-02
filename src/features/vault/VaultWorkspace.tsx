import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useToast } from "../../ui";
import { vaultApi, vaultError, type EntryView } from "./api";
import { DiscoverPanel } from "./DiscoverPanel";
import { EntryDetail } from "./EntryDetail";
import { EntryEditor } from "./EntryEditor";
import { EntryList } from "./EntryList";
import { VaultMenu } from "./VaultMenu";
import { SshKeyGenerator } from "./SshKeys";
import { filterEntries, platformsOf, soonCount } from "./vaultView";

export function VaultWorkspace({ onLocked }: { onLocked: () => void }) {
  const toast = useToast();
  const [tab, setTab] = useState<"entries" | "discover">("entries");
  const [entries, setEntries] = useState<EntryView[]>([]);
  const [viewingId, setViewingId] = useState<string | null>(null);
  const [filter, setFilter] = useState({ query: "", platform: "", soonOnly: false });
  const [editing, setEditing] = useState<EntryView | null | "new">(null);
  const [generating, setGenerating] = useState(false);
  const today = useMemo(() => new Date(), []);

  // Kept in a ref so `load` (and the effect below) never re-run just because the parent re-rendered.
  const lockedRef = useRef(onLocked);
  useEffect(() => { lockedRef.current = onLocked; }, [onLocked]);

  const load = useCallback(async () => {
    try { setEntries(await vaultApi.list(false)); }
    catch (error) {
      if (String(error).includes("E_VAULT_LOCKED")) lockedRef.current();
      else toast(vaultError(error), "err");
    }
  }, [toast]);
  useEffect(() => { void load(); }, [load]);
  // Logins from the browser extension arrive while the page is open.
  useEffect(() => {
    let stop: (() => void) | undefined;
    let disposed = false;
    void listen("vault-inbox", () => { void load(); }).then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; });
    return () => { disposed = true; stop?.(); };
  }, [load]);

  const visible = filterEntries(entries, filter, today);
  const viewing = entries.find((entry) => entry.id === viewingId) ?? null;
  const soon = soonCount(entries, today);

  async function lock() {
    await vaultApi.lock().catch(() => undefined);
    onLocked();
  }

  return (
    <>
      <div className="vault-bar">
        <div className="seg">
          <button className={tab === "entries" ? "on" : ""} onClick={() => setTab("entries")}>条目</button>
          <button className={tab === "discover" ? "on" : ""} onClick={() => setTab("discover")}>发现</button>
        </div>
        <span className="grow" />
        {tab === "entries" && <button className="gh sm" title="在保管库里生成一把新的 SSH 密钥，公钥交给服务器" onClick={() => setGenerating(true)}><i className="ti ti-key" /> 生成 SSH 密钥</button>}
        {tab === "entries" && <button className="pr sm" onClick={() => setEditing("new")}><i className="ti ti-plus" /> 新建</button>}
        <button className="gh sm" onClick={() => void lock()}><i className="ti ti-lock" /> 锁定</button>
        <VaultMenu onChanged={() => void load()} />
      </div>

      {tab === "discover" ? <DiscoverPanel onImported={() => void load()} /> : (
        <>
          {soon > 0 && <div className="callout vault-note"><i className="ti ti-calendar-exclamation" /><div>{soon} 项凭据将在 14 天内到期。</div></div>}
          <div className="vault-bar">
            <input className="ip grow" placeholder="搜索标题、平台、标签、备注" value={filter.query} onChange={(e) => setFilter({ ...filter, query: e.target.value })} />
            <select className="ip" value={filter.platform} onChange={(e) => setFilter({ ...filter, platform: e.target.value })}>
              <option value="">全部平台</option>
              {platformsOf(entries).map((platform) => <option key={platform} value={platform}>{platform}</option>)}
            </select>
            <button className={"gh sm" + (filter.soonOnly ? " on" : "")} onClick={() => setFilter({ ...filter, soonOnly: !filter.soonOnly })}>即将到期</button>
          </div>
          {entries.length === 0 ? (
            <div className="pxcard vault-empty">暂无条目。添加 API Key、令牌或 SSH 密钥，数据加密后仅保存在本机。</div>
          ) : (
            <div className="pxcard">
              <EntryList entries={visible} today={today} onView={(entry) => setViewingId(entry.id)} onEdit={setEditing} onChanged={() => void load()} />
            </div>
          )}
        </>
      )}

      {viewing && !editing && (
        <EntryDetail key={viewing.id} entry={viewing} today={today} onClose={() => setViewingId(null)}
          onEdit={() => setEditing(viewing)} onChanged={() => void load()} />
      )}
      {generating && <SshKeyGenerator onClose={() => setGenerating(false)} onSaved={() => void load()} />}
      {editing && (
        <EntryEditor entry={editing === "new" ? null : editing} onClose={() => setEditing(null)}
          onSaved={() => { setEditing(null); void load(); }} />
      )}
    </>
  );
}
