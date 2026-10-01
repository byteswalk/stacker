import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "../../../invoke";
import { useI18n } from "../../../i18n";
import { ConfirmModal, useToast } from "../../../ui";
import { formatSpaceBytes } from "./SpaceOverview";

export type FileToRemove = { path: string; bytes: number };
type RemovalResult = { removed: number; releasedBytes: number; failures: { path: string; reason: string }[] };

/** The disk cards listen for this to read the volumes again. */
export const VOLUMES_CHANGED = "stacker:volumes-changed";

const REASONS: Record<string, string> = {
  protected: "系统或程序目录里的文件不能在这里删除",
  changed: "文件在扫描后变过，已跳过",
  missing: "文件已经不在了",
};

/** Which of these paths sit under Windows, a program's folder and the like, and stay put. */
export function useProtectedPaths(paths: string[]): Set<string> {
  const [protectedPaths, setProtected] = useState<Set<string>>(new Set());
  const key = paths.join("\n");
  useEffect(() => {
    if (!key) { setProtected(new Set()); return; }
    let alive = true;
    const list = key.split("\n");
    invoke<boolean[]>("space_protected_paths", { paths: list })
      .then((flags) => { if (alive) setProtected(new Set(list.filter((_, i) => flags[i]))); })
      .catch(() => { if (alive) setProtected(new Set()); });
    return () => { alive = false; };
  }, [key]);
  return protectedPaths;
}

/** The bar under a list: how much is picked, and the one button that moves it to the Recycle Bin. */
export function RecycleBar({ files, extra, onDone }: {
  files: FileToRemove[];
  /** Controls of the list's own, shown before the count (select all, smart select). */
  extra?: ReactNode;
  onDone: (removed: string[]) => void;
}) {
  const { tr } = useI18n();
  const toast = useToast();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  // The Recycle Bin keeps the space until it is emptied; deleting outright frees it now.
  const [permanent, setPermanent] = useState(false);
  const total = files.reduce((sum, file) => sum + file.bytes, 0);

  async function run() {
    setBusy(true);
    try {
      const result = await invoke<RemovalResult>("space_recycle_files", { files, permanent });
      const failed = new Set(result.failures.map((f) => f.path));
      onDone(files.map((f) => f.path).filter((path) => !failed.has(path)));
      window.dispatchEvent(new Event(VOLUMES_CHANGED));
      if (!result.failures.length) {
        toast(tr(permanent ? "已彻底删除 {count} 个文件，释放 {size}" : "已移到回收站 {count} 个文件（{size}），清空回收站后释放空间")
          .replace("{count}", String(result.removed)).replace("{size}", formatSpaceBytes(result.releasedBytes)), "ok");
      } else {
        const first = result.failures[0];
        toast(tr("移走 {count} 个，{failed} 个没动：").replace("{count}", String(result.removed)).replace("{failed}", String(result.failures.length))
          + tr(REASONS[first.reason] ?? first.reason), result.removed ? "info" : "err");
      }
    } catch (e) {
      toast(String(e), "err");
    } finally {
      setBusy(false);
      setConfirming(false);
    }
  }

  return <div className="recycle-bar">
    {extra}
    <span className="recycle-bar-count">{files.length
      ? tr("已选 {count} 个 · {size}").replace("{count}", String(files.length)).replace("{size}", formatSpaceBytes(total))
      : tr("勾选要删除的文件")}</span>
    <button className="pr sm" disabled={!files.length || busy} onClick={() => setConfirming(true)}>
      <i className={"ti " + (busy ? "ti-loader spin" : "ti-trash")} /> {tr("删除所选")}{files.length ? ` (${files.length})` : ""}
    </button>
    {confirming && <ConfirmModal title={tr(permanent ? "彻底删除" : "移到回收站")} icon="ti-trash" danger busy={busy}
      confirmLabel={tr(permanent ? "彻底删除" : "移到回收站")}
      message={<>
        <div>{tr(permanent
          ? "彻底删除选中的 {count} 个文件（{size}），不经过回收站，删了就找不回来。"
          : "把选中的 {count} 个文件（{size}）移到回收站。在回收站里还能还原；清空回收站后才真正释放空间。")
          .replace("{count}", String(files.length)).replace("{size}", formatSpaceBytes(total))}</div>
        <label className="recycle-permanent">
          <input type="checkbox" checked={permanent} onChange={(e) => setPermanent(e.target.checked)} />
          {tr("直接彻底删除（不进回收站，立即释放空间）")}
        </label>
      </>}
      onConfirm={() => void run()} onClose={() => setConfirming(false)} />}
  </div>;
}
