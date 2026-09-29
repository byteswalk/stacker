import { useState } from "react";
import { invoke } from "../../../invoke";
import { useI18n } from "../../../i18n";
import { useToast } from "../../../ui";
import { formatSpaceBytes as bytes } from "./SpaceOverview";

type DuplicateGroup = { bytes: number; wasted: number; paths: string[]; verified: boolean };
type DuplicateReport = { groups: DuplicateGroup[]; wasted: number; complete: boolean };

const SIZES: { value: number; label: string }[] = [
  { value: 1024 * 1024, label: "1 MB 以上" },
  { value: 10 * 1024 * 1024, label: "10 MB 以上" },
  { value: 100 * 1024 * 1024, label: "100 MB 以上" },
];

/** Files kept twice, which is the one kind of waste a size ranking cannot show. */
export function DuplicateFiles({ taskId }: { taskId: string }) {
  const { tr: t } = useI18n();
  const toast = useToast();
  const [minBytes, setMinBytes] = useState(SIZES[1].value);
  const [report, setReport] = useState<DuplicateReport | null>(null);
  const [busy, setBusy] = useState(false);

  async function search(min: number) {
    setBusy(true);
    setMinBytes(min);
    try {
      setReport(await invoke<DuplicateReport>("space_duplicates", { taskId, minBytes: min }));
    } catch (e) { toast(String(e), "err"); }
    finally { setBusy(false); }
  }

  const open = (path: string) => void invoke("space_open_directory", { path: path.replace(/[\\/][^\\/]+$/, "") })
    .catch((e) => toast(String(e), "err"));

  return <div className="dupes">
    <div className="dupes-bar">
      <div className="seg sm">
        {SIZES.map((size) => <button key={size.value} className={minBytes === size.value ? "on" : ""}
          disabled={busy} onClick={() => void search(size.value)}>{t(size.label)}</button>)}
      </div>
      <button className="pr sm" disabled={busy} onClick={() => void search(minBytes)}>
        <i className={"ti " + (busy ? "ti-loader spin" : "ti-copy-check")} /> {t(busy ? "正在比对…" : "查找重复文件")}
      </button>
      {report && <span className="s dim">{t("可省出")} {bytes(report.wasted)} · {report.groups.length} {t("组")}</span>}
    </div>
    {!report && !busy && <p className="proxy-note">{t("按大小先筛，再逐字节比对内容；只列出内容完全相同的文件。大于 256 MB 的文件按大小和首尾片段判断，会单独标注。")}</p>}
    {report && !report.groups.length && <div className="space-analysis-state"><i className="ti ti-sparkles" /><span>{t("这次扫描范围里没有重复文件")}</span></div>}
    {report?.groups.map((group, index) => <div className="dupe-group" key={index}>
      <div className="dupe-head">
        <b>{bytes(group.bytes)} × {group.paths.length}</b>
        <span className="green">{t("可省")} {bytes(group.wasted)}</span>
        {!group.verified && <span className="bd y" title={t("文件太大，未逐字节比对")}>{t("按首尾片段判断")}</span>}
      </div>
      {group.paths.map((path) => <div className="dupe-path" key={path}>
        <code title={path}>{path}</code>
        <button className="gh xs" title={t("打开所在目录")} onClick={() => open(path)}><i className="ti ti-folder-open" /></button>
      </div>)}
    </div>)}
    {report && !report.complete && <p className="proxy-note">{t("比对被中断，结果不完整。")}</p>}
  </div>;
}
