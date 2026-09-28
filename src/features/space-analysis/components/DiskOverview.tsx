import { useEffect, useState } from "react";
import { invoke } from "../../../invoke";
import { useI18n } from "../../../i18n";
import type { VolumeInfo } from "../types";
import { formatSpaceBytes as bytes } from "./SpaceOverview";

/** How full a disk is, in the words the page uses about it. */
export function pressure(volume: VolumeInfo): { used: number; ratio: number; cls: string } {
  const used = Math.max(0, volume.totalBytes - volume.freeBytes);
  const ratio = volume.totalBytes > 0 ? used / volume.totalBytes : 0;
  // Windows itself starts warning around a tenth free; the colours follow that.
  const cls = ratio >= 0.9 ? "bad" : ratio >= 0.75 ? "warn" : "ok";
  return { used, ratio, cls };
}

/** The disks on this machine, as the first thing the cleanup page shows. */
export function DiskOverview({ onScan, disabled }: { onScan: (root: string) => void; disabled?: boolean }) {
  const { tr } = useI18n();
  const [volumes, setVolumes] = useState<VolumeInfo[] | null>(null);

  useEffect(() => {
    let alive = true;
    invoke<VolumeInfo[]>("space_fixed_volumes")
      .then((found) => { if (alive) setVolumes(found.filter((v) => v.fixed && v.totalBytes > 0)); })
      .catch(() => { if (alive) setVolumes([]); });
    return () => { alive = false; };
  }, []);

  if (!volumes?.length) return null;
  return <div className="disk-overview" aria-label={tr("本机磁盘")}>
    {volumes.map((volume) => {
      const { used, ratio, cls } = pressure(volume);
      return <button type="button" key={volume.root} className="disk-card" disabled={disabled}
        title={`${tr("分析")} ${volume.root}`} onClick={() => onScan(volume.root)}>
        <span className="disk-card-head">
          <i className="ti ti-device-desktop-analytics" />
          <b>{volume.root.replace(/\\$/, "")}</b>
          {volume.label && <em>{volume.label}</em>}
          <span>{Math.round(ratio * 100)}%</span>
        </span>
        <span className="disk-bar"><i className={cls} style={{ width: `${Math.min(100, ratio * 100)}%` }} /></span>
        <span className="disk-card-foot">
          <span>{bytes(used)} / {bytes(volume.totalBytes)}</span>
          <em>{tr("剩余")} {bytes(volume.freeBytes)}</em>
        </span>
      </button>;
    })}
  </div>;
}
