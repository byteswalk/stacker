import { useCallback, useEffect, useState } from "react";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { ConfirmModal, Modal, useToast } from "../../ui";
import { formatSpaceBytes as bytes } from "../space-analysis/components/SpaceOverview";
import { useDragPick, setInArray } from "../../dragPick";

type JunkKind = "rebuildable" | "releases" | "cache" | "environment";
type JunkItem = {
  id: string; kind: JunkKind; label: string; explain: string;
  path: string; bytes: number; files: number; recommended: boolean;
};
type JunkReport = { root: string; items: JunkItem[]; total: number; recommended: number; scannedAt: number };
type Cleaned = { removed: string[]; failed: string[]; bytes: number };

const KIND: Record<JunkKind, { label: string; icon: string }> = {
  rebuildable: { label: "可重新构建", icon: "ti-tools" },
  cache: { label: "缓存与日志", icon: "ti-database" },
  releases: { label: "历史发布版本", icon: "ti-package" },
  environment: { label: "虚拟环境", icon: "ti-box" },
};

const ORDER: JunkKind[] = ["rebuildable", "cache", "releases", "environment"];

/** What one project keeps that can be made again, and what the user wants gone. */
export function ProjectCleanupDialog({ name, path, onClose }: { name: string; path: string; onClose: (changed: boolean) => void }) {
  const { tr: t } = useI18n();
  const toast = useToast();
  const [report, setReport] = useState<JunkReport | null>(null);
  const [picked, setPicked] = useState<string[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(true);
  const [confirming, setConfirming] = useState(false);
  const [changed, setChanged] = useState(false);

  const scan = useCallback(async () => {
    setBusy(true);
    setError("");
    try {
      const found = await invoke<JunkReport>("project_junk_scan", { path });
      setReport(found);
      setPicked(found.items.filter((i) => i.recommended).map((i) => i.id));
    } catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }, [path]);

  useEffect(() => { void scan(); }, [scan]);

  async function clean() {
    setConfirming(false);
    setBusy(true);
    try {
      const done = await invoke<Cleaned>("project_junk_clean", { path, ids: picked });
      toast(`${t("已释放")} ${bytes(done.bytes)}${done.failed.length ? `，${done.failed.length} ${t("项未能删除")}` : ""}`, done.failed.length ? "err" : "ok");
      setChanged(true);
      await scan();
    } catch (e) { setError(String(e)); setBusy(false); }
  }

  const items = report?.items ?? [];
  // The rows in the order shown: grouped by kind.
  const ordered = ORDER.flatMap((kind) => items.filter((i) => i.kind === kind));
  const pick = useDragPick(ordered.map((i) => busy ? [] : [i.id]), (id) => picked.includes(id), (ids, on) => setPicked((old) => setInArray(old, ids, on)));
  const chosen = items.filter((i) => picked.includes(i.id));
  const chosenBytes = chosen.reduce((sum, i) => sum + i.bytes, 0);

  return <>
    <Modal wide icon="ti-recycle" title={<>{t("清理项目")} · {name}</>} onClose={() => onClose(changed)}
      sub={<code title={path}>{path}</code>}
      footer={<>
        <button className="gh sm" disabled={busy} onClick={() => void scan()}><i className={"ti " + (busy ? "ti-loader spin" : "ti-refresh")} /> {t("重新分析")}</button>
        <button className="pr sm" disabled={busy || !chosen.length} onClick={() => setConfirming(true)}>
          <i className="ti ti-trash" /> {t("删除所选")}（{chosen.length}{chosen.length ? ` · ${bytes(chosenBytes)}` : ""}）
        </button>
      </>}>
      {error && <p role="alert" className="session-error">{t(error === "E_PROJECT_MISSING" ? "项目目录已不存在。" : error)}</p>}
      {busy && !report && <div className="session-empty"><i className="ti ti-loader spin" /><b>{t("正在分析项目目录…")}</b></div>}
      {report && !items.length && <div className="session-empty"><i className="ti ti-sparkles" /><b>{t("没有找到可清理的内容")}</b></div>}
      {!!items.length && <>
        <p className="proxy-note">{t("只看目录名和项目类型判断，不读源码；勾选的默认是重新构建或重新安装就能恢复的部分。")}</p>
        {ORDER.filter((kind) => items.some((i) => i.kind === kind)).map((kind) => <div key={kind} className="pj-group">
          <div className="pj-group-head">
            <i className={"ti " + KIND[kind].icon} /> {t(KIND[kind].label)}
            <span>{bytes(items.filter((i) => i.kind === kind).reduce((sum, i) => sum + i.bytes, 0))}</span>
          </div>
          {items.filter((i) => i.kind === kind).map((item) => <label key={item.id} className={"pj-item" + (picked.includes(item.id) ? " on" : "")} {...pick.row(ordered.indexOf(item))}>
            <input type="checkbox" checked={picked.includes(item.id)} disabled={busy} title={t("按住拖过几行可以一起勾选；按住 Shift 点选一段")} {...pick.box(ordered.indexOf(item))} onChange={(e) => pick.change(ordered.indexOf(item), e.target.checked)} />
            <span className="pj-item-tx">
              <b>{item.id}</b>
              <small>{t(item.label)} · {t(item.explain)}</small>
            </span>
            <span className="pj-item-sz">{bytes(item.bytes)}<small>{item.files} {t("个文件")}</small></span>
          </label>)}
        </div>)}
      </>}
    </Modal>
    {confirming && <ConfirmModal danger icon="ti-trash" title={t("删除所选内容")}
      message={<>
        {t("将从磁盘上删除以下内容，不进回收站：")}
        <ul className="pj-confirm">{chosen.map((i) => <li key={i.id}><code>{i.id}</code> <em>{bytes(i.bytes)}</em></li>)}</ul>
        {t("共")} {bytes(chosenBytes)}。{t("源码和 .git 不会被动到。")}
      </>}
      confirmLabel={t("删除")} onConfirm={() => void clean()} onClose={() => setConfirming(false)} />}
  </>;
}
