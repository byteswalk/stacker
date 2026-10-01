import { useEffect, useState } from "react";
import { invoke } from "../../../invoke";
import { useI18n } from "../../../i18n";
import type { VolumeInfo } from "../types";
import { formatSpaceBytes as bytes } from "./SpaceOverview";
import { listen } from "@tauri-apps/api/event";
import { VOLUMES_CHANGED } from "./FileRemoval";

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

  // Free space changes under the page: files deleted here or elsewhere, a cleanup finishing.
  // The disks are read again when that happens, when the window comes back, and every
  // 15 seconds while the page is in view.
  useEffect(() => {
    let alive = true;
    const load = () => {
      invoke<VolumeInfo[]>("space_fixed_volumes")
        .then((found) => { if (alive) setVolumes(found.filter((v) => v.fixed && v.totalBytes > 0)); })
        .catch(() => { if (alive) setVolumes((old) => old ?? []); });
    };
    load();
    const visible = () => { if (document.visibilityState === "visible") load(); };
    const timer = window.setInterval(visible, 15_000);
    window.addEventListener("focus", load);
    window.addEventListener(VOLUMES_CHANGED, load);
    document.addEventListener("visibilitychange", visible);
    let stopCleanup: (() => void) | undefined;
    void listen<{ state?: string }>("space-cleanup-progress", (event) => {
      if (["completed", "cancelled", "failed"].includes(event.payload?.state ?? "")) load();
    }).then((stop) => { if (alive) stopCleanup = stop; else stop(); }).catch(() => undefined);
    return () => {
      alive = false;
      window.clearInterval(timer);
      window.removeEventListener("focus", load);
      window.removeEventListener(VOLUMES_CHANGED, load);
      document.removeEventListener("visibilitychange", visible);
      stopCleanup?.();
    };
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
