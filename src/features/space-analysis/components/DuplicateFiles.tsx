import { useState, useSyncExternalStore } from "react";
import { invoke } from "../../../invoke";
import { useI18n } from "../../../i18n";
import { useToast } from "../../../ui";
import { formatSpaceBytes as bytes } from "./SpaceOverview";
import { FILES_GONE, RecycleBar, useProtectedPaths } from "./FileRemoval";
import { useDragPick } from "../../../dragPick";

type DuplicateGroup = { bytes: number; wasted: number; paths: string[]; verified: boolean };
type DuplicateReport = { groups: DuplicateGroup[]; wasted: number; complete: boolean };
type FileTimes = { created: number; modified: number };

/**
 * The last search, kept outside the page: going to another tab and back shows it again, and a
 * search still running when the tab is left lands here when it ends. A new scan starts afresh.
 */
type Search = { taskId: string; minBytes: number; report: DuplicateReport | null; busy: boolean; picked: Set<string>; times: Map<string, FileTimes> };
let search: Search = { taskId: "", minBytes: 10 * 1024 * 1024, report: null, busy: false, picked: new Set(), times: new Map() };
const listeners = new Set<() => void>();
function setSearch(change: Partial<Search> | ((old: Search) => Partial<Search>)) {
  const next = typeof change === "function" ? change(search) : change;
  // Another scan's results never carry over.
  const base = next.taskId && next.taskId !== search.taskId
    ? { ...search, taskId: next.taskId, report: null, busy: false, picked: new Set<string>(), times: new Map<string, FileTimes>() }
    : search;
  search = { ...base, ...next };
  listeners.forEach((listener) => listener());
}
/** Drops files (or everything under folders) that left the disk, wherever that happened. */
function forget(paths: string[]) {
  if (!paths.length) return;
  const gone = paths.map((path) => path.toLowerCase());
  const isGone = (path: string) => {
    const lower = path.toLowerCase();
    return gone.some((g) => lower === g || lower.startsWith(g.endsWith("\\") ? g : g + "\\"));
  };
  setSearch((old) => {
    const picked = new Set([...old.picked].filter((path) => !isGone(path)));
    if (!old.report) return { picked };
    const groups = old.report.groups
      .map((group) => ({ ...group, paths: group.paths.filter((path) => !isGone(path)) }))
      .filter((group) => group.paths.length > 1)
      .map((group) => ({ ...group, wasted: group.bytes * (group.paths.length - 1) }));
    return { picked, report: { ...old.report, groups, wasted: groups.reduce((sum, group) => sum + group.wasted, 0) } };
  });
}
if (typeof window !== "undefined") {
  window.addEventListener(FILES_GONE, (event) => forget((event as CustomEvent<string[]>).detail ?? []));
}
function useSearch(taskId: string): Search {
  const state = useSyncExternalStore((listener) => { listeners.add(listener); return () => { listeners.delete(listener); }; }, () => search);
  return state.taskId === taskId ? state : { ...state, taskId, report: null, busy: false, picked: new Set(), times: new Map() };
}
/** Forgets the last search (for tests). */
export function resetDuplicateSearch() {
  search = { taskId: "", minBytes: 10 * 1024 * 1024, report: null, busy: false, picked: new Set(), times: new Map() };
}

function when(ms: number | undefined): string {
  if (!ms) return "—";
  const date = new Date(ms);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

const SIZES: { value: number; label: string }[] = [
  { value: 1024 * 1024, label: "1 MB 以上" },
  { value: 10 * 1024 * 1024, label: "10 MB 以上" },
  { value: 100 * 1024 * 1024, label: "100 MB 以上" },
];

/** Where a copy looks like the spare one: a download, a temp file, a backup, an updater's cache. */
const SPARE = /\\(downloads?|temp|tmp|cache|backup|bak|pending)\\|\.(bak|old|tmp)$|\(\d+\)\.\w+$|副本|copy/i;
/** The user's own folders, where a second copy is the user's and not a program's. */
const USER_CONTENT = /\\users\\[^\\]+\\(downloads|desktop|documents|pictures|videos|music|onedrive[^\\]*)\\/i;

/** Where programs keep their own files, or what a program file looks like. */
const PROGRAM = /\\(appdata|program files[^\\]*|programdata|windows|node_modules|\.git|site-packages|\.cargo|\.rustup|\.m2|\.gradle|\.nuget|go\\pkg)\\|\.(dll|exe|sys|so|dylib|jar|pyd|node|msi)$/i;

/**
 * Which copies of one group to pick. A program's files (a CLI's binary in two installs, a DLL
 * two apps ship) are never picked for the user, however identical: in a group with any of
 * them, only copies that look spare or sit in the user's own folders are candidates. A group
 * of plain files (videos, archives, documents) is all candidates. One copy of every group
 * stays: a copy that is not a candidate, or else the one that looks least like a spare, then
 * the shortest path. Groups compared only by their ends are left alone until read in full.
 */
export function smartPick(group: DuplicateGroup, locked: Set<string>): string[] {
  if (!group.verified) return [];
  const plain = !group.paths.some((path) => PROGRAM.test(path));
  const candidates = group.paths.filter((path) => !locked.has(path) && (plain || SPARE.test(path) || USER_CONTENT.test(path)));
  if (candidates.length < group.paths.length) return candidates;
  const keep = [...candidates].sort((a, b) =>
    Number(SPARE.test(a)) - Number(SPARE.test(b)) || a.length - b.length || a.localeCompare(b))[0];
  return candidates.filter((path) => path !== keep);
}

/** Files kept twice, which is the one kind of waste a size ranking cannot show. */
export function DuplicateFiles({ taskId }: { taskId: string }) {
  const { tr: t } = useI18n();
  const toast = useToast();
  const { minBytes, report, busy, picked, times } = useSearch(taskId);
  const [verifying, setVerifying] = useState<[number, number] | null>(null);
  const locked = useProtectedPaths(report?.groups.flatMap((g) => g.paths) ?? []);
  const setReport = (next: DuplicateReport | null) => setSearch({ taskId, report: next });
  const setPicked = (next: Set<string> | ((old: Set<string>) => Set<string>)) =>
    setSearch((old) => ({ taskId, picked: typeof next === "function" ? next(old.taskId === taskId ? old.picked : new Set()) : next }));

  // The size only says what the next search looks at; nothing runs until the button is pressed.
  async function find() {
    const min = minBytes;
    setSearch({ taskId, minBytes: min, busy: true, picked: new Set() });
    try {
      const found = await invoke<DuplicateReport>("space_duplicates", { taskId, minBytes: min });
      if (search.taskId !== taskId) return;
      setSearch({ report: found, busy: false });
      const paths = found.groups.flatMap((group) => group.paths);
      const read = await invoke<FileTimes[]>("space_file_times", { paths }).catch(() => [] as FileTimes[]);
      if (search.taskId === taskId && search.report === found) setSearch({ times: new Map(paths.map((path, index) => [path, read[index]])) });
    } catch (e) {
      toast(String(e), "err");
      if (search.taskId === taskId) setSearch({ busy: false });
    }
  }

  const open = (path: string) => void invoke("space_open_directory", { path: path.replace(/[\\/][^\\/]+$/, "") })
    .catch((e) => toast(String(e), "err"));

  // One copy of every group always stays: the last unpicked one cannot be picked, however it is pressed.
  const flat = (report?.groups ?? []).flatMap((group) => group.paths.map((path) => ({ group, path })));
  const order = new Map(flat.map((item, index) => [item.path, index]));
  const pick = useDragPick(flat.map(({ path, group }) => locked.has(path) || !group.verified ? [] : [path]), (path) => picked.has(path), (paths, on) => setPicked((old) => {
    const next = new Set(old);
    let kept = false;
    for (const path of paths) {
      if (!on) { next.delete(path); continue; }
      const group = flat[order.get(path) ?? -1]?.group;
      if (group && group.paths.filter((p) => p !== path && !next.has(p)).length === 0) { kept = true; continue; }
      next.add(path);
    }
    if (kept) toast(t("每组至少保留一份"), "info");
    return next;
  }));

  // Groups judged by their ends are read in full first; what proves identical is then picked like the rest.
  async function pickAll() {
    if (!report) return;
    const pending = report.groups.filter((group) => !group.verified);
    let groups = report.groups;
    if (pending.length) {
      setVerifying([0, pending.length]);
      const current = report;
      try {
        const settled = new Map<DuplicateGroup, DuplicateGroup[]>();
        for (const [index, group] of pending.entries()) {
          const sets = await invoke<string[][]>("space_verify_duplicates", { paths: group.paths });
          settled.set(group, sets.map((paths) => ({ bytes: group.bytes, wasted: group.bytes * (paths.length - 1), paths, verified: true })));
          setVerifying([index + 1, pending.length]);
        }
        groups = report.groups.flatMap((group) => settled.get(group) ?? [group]).sort((a, b) => b.wasted - a.wasted);
        setReport({ ...current, groups, wasted: groups.reduce((sum, group) => sum + group.wasted, 0) });
      } catch (e) {
        toast(String(e), "err");
      } finally {
        setVerifying(null);
      }
    }
    setPicked(new Set(groups.flatMap((group) => smartPick(group, locked))));
  }

  function removed(paths: string[]) {
    const gone = new Set(paths);
    setPicked((old) => new Set([...old].filter((path) => !gone.has(path))));
    if (!report) return;
    const groups = report.groups
      .map((group) => ({ ...group, paths: group.paths.filter((path) => !gone.has(path)) }))
      .filter((group) => group.paths.length > 1)
      .map((group) => ({ ...group, wasted: group.bytes * (group.paths.length - 1) }));
    setReport({ ...report, groups, wasted: groups.reduce((sum, group) => sum + group.wasted, 0) });
  }

  const size = new Map((report?.groups ?? []).flatMap((group) => group.paths.map((path) => [path, group.bytes] as const)));
  const files = [...picked].map((path) => ({ path, bytes: size.get(path) ?? 0 }));

  return <div className="dupes">
    <div className="dupes-bar">
      <div className="seg sm">
        {SIZES.map((option) => <button key={option.value} className={minBytes === option.value ? "on" : ""}
          disabled={busy} onClick={() => setSearch({ taskId, minBytes: option.value })}>{t(option.label)}</button>)}
      </div>
      <button className="pr sm" disabled={busy} onClick={() => void find()}>
        <i className={"ti " + (busy ? "ti-loader spin" : "ti-copy-check")} /> {t(busy ? "正在比对…" : "查找重复文件")}
      </button>
      {report && <span className="s dim">{t("可省出")} {bytes(report.wasted)} · {report.groups.length} {t("组")}</span>}
    </div>
    {!report && !busy && <p className="proxy-note">{t("按大小先筛，再逐字节比对内容；只列出内容完全相同的文件。大于 256 MB 的文件按大小和首尾片段判断，会单独标注。")}</p>}
    {report && !report.groups.length && <div className="space-analysis-state"><i className="ti ti-sparkles" /><span>{t("这次扫描范围里没有重复文件")}</span></div>}
    {!!report?.groups.length && <div className="dupe-path dupe-columns" aria-hidden="true">
      <span className="recycle-check" /><span className="dupe-col-path">{t("路径")}</span>
      <span className="dupe-time">{t("创建时间")}</span><span className="dupe-time">{t("修改时间")}</span><span className="dupe-col-op" />
    </div>}
    {report?.groups.map((group, index) => <div className="dupe-group" key={index}>
      <div className="dupe-head">
        <b>{bytes(group.bytes)} × {group.paths.length}</b>
        <span className="green">{t("可省")} {bytes(group.wasted)}</span>
        {!group.verified && <span className="bd y" title={t("文件太大，未逐字节比对")}>{t("按首尾片段判断")}</span>}
      </div>
      {group.paths.map((path) => <div className={"dupe-path" + (picked.has(path) ? " picked" : "")} key={path} {...pick.row(order.get(path) ?? -1)}>
        {locked.has(path)
          ? <span className="recycle-lock" title={t("系统或程序目录里的文件不能在这里删除")}><i className="ti ti-lock" /></span>
          : !group.verified
          ? <span className="recycle-lock" title={t("只比对过首尾片段，先点「智能勾选」逐字节核对，确认完全一致后才能选")}><i className="ti ti-lock-question" /></span>
          : <input type="checkbox" className="recycle-check" checked={picked.has(path)} aria-label={t("选择")} title={t("按住拖过几行可以一起勾选；按住 Shift 点选一段")}
            {...pick.box(order.get(path) ?? -1)} onChange={(e) => pick.change(order.get(path) ?? -1, e.target.checked)} />}
        <code title={path}>{path}</code>
        <span className="dupe-time" title={t("创建时间")}>{when(times.get(path)?.created)}</span>
        <span className="dupe-time" title={t("修改时间")}>{when(times.get(path)?.modified)}</span>
        <button className="gh xs" title={t("打开所在目录")} onClick={() => open(path)}><i className="ti ti-folder-open" /></button>
      </div>)}
    </div>)}
    {!!report?.groups.length && <RecycleBar files={files} onDone={removed}
      extra={<>
        <button className="gh sm" disabled={!!verifying} onClick={() => void pickAll()} title={t("每组保留一份。只比对过首尾的大文件先逐字节核对，内容完全一致才勾；有程序文件的组（AppData、node_modules、dll、exe 等）只勾临时、备份、副本或你自己文件夹里的那份；系统目录里的不勾")}>
          <i className={"ti " + (verifying ? "ti-loader spin" : "ti-wand")} /> {verifying
            ? t("正在逐字节核对 {done}/{total} 组…").replace("{done}", String(verifying[0])).replace("{total}", String(verifying[1]))
            : t("智能选择")}
        </button>
        {picked.size > 0 && <button className="gh sm" onClick={() => setPicked(new Set())}>{t("清空选择")}</button>}
      </>} />}
    {report && !report.complete && <p className="proxy-note">{t("比对被中断，结果不完整。")}</p>}
  </div>;
}
