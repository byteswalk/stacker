import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useToast } from "../../ui";
import { invoke } from "../../invoke";
import { aiError } from "../ai/AiSettings";
import { AiSetupPrompt, needsAiSetup } from "../ai/AiAsk";
import { vaultApi, vaultError, type EntryView } from "./api";
import { DiscoverPanel } from "./DiscoverPanel";
import { EntryDetail } from "./EntryDetail";
import { EntryEditor } from "./EntryEditor";
import { EntryList } from "./EntryList";
import { VaultMenu } from "./VaultMenu";
import { SshKeyGenerator } from "./SshKeys";
import { CATEGORY_LABELS, CATEGORY_ORDER, categoryOf, filterEntries, platformsOf, soonCount, type Category, type ListFilter } from "./vaultView";

const EMPTY: ListFilter = { query: "", platform: "", soonOnly: false, kind: "", windows: "" };

export function VaultWorkspace({ onLocked }: { onLocked: () => void }) {
  const toast = useToast();
  const [tab, setTab] = useState<"entries" | "discover">("entries");
  const [entries, setEntries] = useState<EntryView[]>([]);
  const [viewingId, setViewingId] = useState<string | null>(null);
  const [filter, setFilter] = useState<ListFilter>(EMPTY);
  const [finding, setFinding] = useState(false);
  const [aiSetup, setAiSetup] = useState<unknown>(null);
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

  // A website password's platform is its site, which the search already finds; the AI is
  // told only the platforms of keys and SSH keys, so the sites one logs into stay here.
  const platforms = platformsOf(entries.filter((entry) => categoryOf(entry) !== "web"));
  // The words in the search box become the filters beside it, where they can be read and undone.
  // Only the words and those platform names go to the AI, never a title, an account or a value.
  async function findWithAi() {
    const words = filter.query.trim();
    if (!words) { toast("先在搜索框里用一句话描述要找的条目，比如“阿里云上快到期的 API 密钥”", "info"); return; }
    setFinding(true);
    try {
      const found = await invoke<{ search: string; kind: string; platform: string; windows: string; soonOnly: boolean }>("ai_vault_filter", { query: words, platforms });
      setFilter({
        query: found.search, platform: found.platform, soonOnly: found.soonOnly,
        kind: found.kind as ListFilter["kind"], windows: found.windows as ListFilter["windows"],
      });
    } catch (error) { if (needsAiSetup(error)) setAiSetup(error); else toast(aiError(error), "err"); }
    finally { setFinding(false); }
  }
  const filtered = filter.platform !== "" || filter.kind !== "" || filter.windows !== "" || filter.soonOnly;

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
            <input className="ip grow" placeholder="搜索标题、网站、账号、标签、备注" value={filter.query}
              onChange={(e) => setFilter({ ...filter, query: e.target.value })}
              onKeyDown={(e) => { if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) void findWithAi(); }} />
            <button className="gh sm ai-btn" disabled={finding} title="把搜索框里的一句话交给 AI，换成右边这些筛选条件；只发送这句话和平台名，不含任何账号和密钥（Ctrl+Enter）"
              onClick={() => void findWithAi()}>
              <i className={"ti " + (finding ? "ti-loader spin" : "ti-sparkles")} /> AI 查找
            </button>
            <select className="ip" value={filter.kind ?? ""} onChange={(e) => setFilter({ ...filter, kind: e.target.value as ListFilter["kind"] })}>
              <option value="">全部类型</option>
              {CATEGORY_ORDER.map((category: Category) => <option key={category} value={category}>{CATEGORY_LABELS[category]}</option>)}
            </select>
            {filter.platform && <button className="gh sm on" title="只看这个平台的条目，点一下取消" onClick={() => setFilter({ ...filter, platform: "" })}>
              <span translate="no">{filter.platform}</span> <i className="ti ti-x" />
            </button>}
            <select className="ip" style={{ width: 170 }} value={filter.windows ?? ""} onChange={(e) => setFilter({ ...filter, windows: e.target.value as ListFilter["windows"] })}>
              <option value="">Windows 凭据：全部</option>
              <option value="on">已放进</option>
              <option value="off">未放进</option>
            </select>
            <button className={"gh sm" + (filter.soonOnly ? " on" : "")} onClick={() => setFilter({ ...filter, soonOnly: !filter.soonOnly })}>即将到期</button>
            {filtered && <button className="gh sm" title="清除全部筛选条件" onClick={() => setFilter({ ...EMPTY, query: filter.query })}><i className="ti ti-filter-off" /> 清除筛选</button>}
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

      {aiSetup !== null && <AiSetupPrompt error={aiSetup} onClose={() => setAiSetup(null)} />}
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
