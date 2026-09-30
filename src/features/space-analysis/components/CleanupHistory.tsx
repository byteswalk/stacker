import { useEffect, useState } from "react";
import { invoke } from "../../../invoke";
import { useI18n } from "../../../i18n";
import { ConfirmModal, Modal, useToast } from "../../../ui";
import { cleanupLabel } from "./CleanupResultModal";
import { formatSpaceBytes } from "./DevelopmentArtifacts";

type HistoryItem = { path: string; state: string; releasedBytes: number; reasonKey: string | null };
type HistoryRecord = { at: number; state: string; releasedBytes: number; items: HistoryItem[] };

/** Every cleanup that ran, so "what did I delete last week" has an answer. */
export function CleanupHistory({ onClose }: { onClose: () => void }) {
  const { tr, locale } = useI18n();
  const toast = useToast();
  const [records, setRecords] = useState<HistoryRecord[] | null>(null);
  const [open, setOpen] = useState<number | null>(0);
  const [confirmClear, setConfirmClear] = useState(false);

  useEffect(() => {
    invoke<HistoryRecord[]>("space_cleanup_history").then(setRecords).catch(() => setRecords([]));
  }, []);

  async function clear() {
    try {
      await invoke("space_cleanup_history_clear");
      setRecords([]);
      toast(tr("清理记录已清空"), "ok");
    } catch (error) {
      toast(String(error), "err");
    }
  }

  const total = (records ?? []).reduce((sum, record) => sum + record.releasedBytes, 0);

  return <Modal wide title={tr("清理记录")} icon="ti-history"
    sub={tr("每次清理完成后记一笔：时间、每一项的结果和释放的空间。只记路径和大小，不记文件内容。")}
    onClose={onClose}
    footer={<>
      <button className="gh sm" disabled={!records?.length} onClick={() => setConfirmClear(true)}><i className="ti ti-trash" /> {tr("清空记录")}</button>
      <button className="pr sm" onClick={onClose}>{tr("关闭")}</button>
    </>}>
    {records === null && <div className="space-analysis-empty"><i className="ti ti-loader spin" /> {tr("正在读取…")}</div>}
    {records?.length === 0 && <div className="space-analysis-empty"><i className="ti ti-history" /> {tr("还没有清理过。")}</div>}
    {!!records?.length && <>
      <div className="clean-history-sum">{tr("共")} {records.length} {tr("次清理，累计释放")} <b>{formatSpaceBytes(total)}</b></div>
      <div className="clean-history">
        {records.map((record, index) => {
          const done = record.items.filter((item) => item.state === "completed").length;
          return <div className={"clean-history-row" + (open === index ? " open" : "")} key={`${record.at}-${index}`}>
            <button className="clean-history-head" onClick={() => setOpen(open === index ? null : index)}>
              <i className={"ti " + (open === index ? "ti-chevron-down" : "ti-chevron-right")} />
              <span className="when">{new Date(record.at * 1000).toLocaleString(locale)}</span>
              <span className={"bd " + (record.state === "completed" ? "g" : record.state === "failed" ? "r" : "n")}>{tr(cleanupLabel(record.state))}</span>
              <span className="s dim">{done} / {record.items.length} {tr("项")}</span>
              <b>{formatSpaceBytes(record.releasedBytes)}</b>
            </button>
            {open === index && <div className="clean-history-items">
              {record.items.map((item) => <div key={item.path}>
                <i className={"ti " + (item.state === "completed" ? "ti-circle-check" : item.state === "failed" ? "ti-alert-circle" : "ti-info-circle")} />
                <span className="mono" title={item.path}>{item.path}</span>
                <small>{tr(cleanupLabel(item.reasonKey ?? item.state))}</small>
                <b>{formatSpaceBytes(item.releasedBytes)}</b>
              </div>)}
            </div>}
          </div>;
        })}
      </div>
    </>}
    {confirmClear && <ConfirmModal title={tr("清空清理记录")} icon="ti-trash" danger
      message={tr("只删除这份记录，不影响任何文件。确定吗？")} confirmLabel={tr("清空")}
      onConfirm={() => { setConfirmClear(false); void clear(); }} onClose={() => setConfirmClear(false)} />}
  </Modal>;
}
