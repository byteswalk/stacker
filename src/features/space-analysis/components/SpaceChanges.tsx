import { useEffect, useState } from "react";
import { invoke } from "../../../invoke";
import { useI18n } from "../../../i18n";
import type { SnapshotComparison, SnapshotMetadata } from "../types";
import { latestComparablePair, signedBytes } from "../changeModel";
import { formatSpaceBytes } from "./DevelopmentArtifacts";

export function SpaceChanges({ fingerprint }: { fingerprint: string }) {
  const { tr } = useI18n();
  const [comparison, setComparison] = useState<SnapshotComparison | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let current = true;
    setLoading(true); setError(null); setComparison(null);
    void invoke<SnapshotMetadata[]>("space_snapshot_list").then(async (items) => {
      const pair = latestComparablePair(items, fingerprint);
      if (!pair) return null;
      return invoke<SnapshotComparison>("space_snapshot_compare", { baseId: pair.base.id, currentId: pair.current.id, offset: 0, limit: 500 });
    }).then((value) => { if (current) setComparison(value); }).catch((cause) => { if (current) setError(String(cause)); }).finally(() => { if (current) setLoading(false); });
    return () => { current = false; };
  }, [fingerprint]);
  if (loading) return <div className="space-analysis-state"><i className="ti ti-loader spin" />{tr("正在读取空间变化…")}</div>;
  if (error) return <div className="space-analysis-state error"><i className="ti ti-alert-triangle" />{tr("无法读取空间变化记录。")}</div>;
  if (!comparison) return <div className="space-analysis-empty">{tr("这是当前目标的首份快照。完成下一次扫描后即可查看空间变化。")}</div>;
  return <>
    <div className="space-change-summary"><div><span>{tr("总占用变化")}</span><b className={comparison.deltaBytes > 0 ? "grow" : "shrink"}>{signedBytes(comparison.deltaBytes, formatSpaceBytes)}</b></div>
      <div><span>{tr("对比时间")}</span><b>{new Date(comparison.base.createdAt).toLocaleString()} → {new Date(comparison.current.createdAt).toLocaleString()}</b></div></div>
    <div className="space-change-list">{comparison.changes.items.map((row) => <div key={row.relativePath}><span title={row.relativePath}>{row.relativePath}</span><small>{formatSpaceBytes(row.beforeBytes)} → {formatSpaceBytes(row.afterBytes)}</small><b className={row.deltaBytes > 0 ? "grow" : "shrink"}>{signedBytes(row.deltaBytes, formatSpaceBytes)}</b></div>)}</div>
    {comparison.changes.total === 0 && <div className="space-analysis-empty">{tr("两次扫描之间没有目录占用变化。")}</div>}
  </>;
}
