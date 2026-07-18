import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { useI18n } from "../../../i18n";
import { invoke } from "../../../invoke";
import { Modal, operationWasCancelled, useToast } from "../../../ui";
import { startScan, useSpaceScan } from "../store";
import {
  loadRememberedTargets,
  rememberStartedScan,
} from "../targetStore";
import type { ScanRequest, ScanTaskState, VolumeInfo } from "../types";
import {
  createDiskSelectorState,
  startAndRememberScan,
  type DiskSelectorKind,
} from "../launcherViewModel";

type SpaceAnalysisSettings = {
  remember_scan_targets?: boolean;
};

function formatBytes(bytes: number) {
  if (bytes >= 1024 ** 4) return `${(bytes / 1024 ** 4).toFixed(1)} TB`;
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(0)} MB`;
  return `${bytes} B`;
}

function isActiveScan(state: ScanTaskState | undefined) {
  return state === "queued" || state === "running" || state === "cancelling";
}

export function ScanLauncher({ disabled = false }: { disabled?: boolean }) {
  const { tr } = useI18n();
  const toast = useToast();
  const scan = useSpaceScan();
  const [rememberTargets, setRememberTargets] = useState(false);
  const [busy, setBusy] = useState(false);
  const [selector, setSelector] = useState<DiskSelectorKind | null>(null);
  const [volumes, setVolumes] = useState<VolumeInfo[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [volumeLoading, setVolumeLoading] = useState(false);
  const [volumeError, setVolumeError] = useState(false);
  const active = isActiveScan(scan.progress?.state);
  const controlsDisabled = disabled || busy || active;

  useEffect(() => {
    invoke<SpaceAnalysisSettings>("settings_get")
      .then((settings) => setRememberTargets(settings.remember_scan_targets ?? true))
      .catch(() => setRememberTargets(false));
  }, []);

  async function launch(request: ScanRequest) {
    setBusy(true);
    try {
      const outcome = await startAndRememberScan(request, rememberTargets, {
        start: startScan,
        remember: (acceptedRequest, remember) => rememberStartedScan(
          acceptedRequest.mode,
          acceptedRequest.targets,
          remember,
        ),
      });
      if (!outcome.memory.ok) {
        toast(tr("扫描已开始，但无法保存扫描目标。"), "info");
      }
      setSelector(null);
    } catch {
      toast(tr("无法启动扫描，请重试。"), "err");
    } finally {
      setBusy(false);
    }
  }

  async function chooseFolder() {
    try {
      const remembered = loadRememberedTargets("directories");
      const chosen = await open({
        directory: true,
        multiple: false,
        defaultPath: remembered[0],
        title: tr("选择要分析的目录"),
      });
      const directory = Array.isArray(chosen) ? chosen[0] : chosen;
      if (typeof directory === "string" && directory) {
        await launch({ mode: "directories", targets: [directory] });
      }
    } catch (error) {
      if (!operationWasCancelled(error)) toast(tr("无法打开目录选择器，请重试。"), "err");
    }
  }

  async function openDiskSelector(kind: DiskSelectorKind) {
    setSelector(kind);
    setVolumes([]);
    setSelected(new Set());
    setVolumeError(false);
    setVolumeLoading(true);
    try {
      const available = await invoke<VolumeInfo[]>("space_fixed_volumes");
      const remembered = kind === "drives" ? loadRememberedTargets("drives") : [];
      const state = createDiskSelectorState(kind, available, remembered);
      setVolumes(state.volumes);
      setSelected(new Set(state.selected));
    } catch {
      setVolumeError(true);
    } finally {
      setVolumeLoading(false);
    }
  }

  function toggleVolume(root: string) {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(root)) next.delete(root);
      else next.add(root);
      return next;
    });
  }

  const entries = [
    { label: "快速扫描", icon: "ti-bolt", action: () => launch({ mode: "quick", targets: [] }) },
    { label: "选择目录", icon: "ti-folder-open", action: chooseFolder },
    { label: "选择磁盘", icon: "ti-device-hdd", action: () => openDiskSelector("drives") },
    { label: "全盘分析", icon: "ti-chart-treemap", action: () => openDiskSelector("all") },
  ];

  return (
    <>
      <section className="scan-launcher" aria-label={tr("选择扫描范围")}>
        <div className="scan-launcher-copy">
          <strong>{tr("开始空间分析")}</strong>
          <span>{tr("选择快速检查、目录或本地固定磁盘。扫描仅在手动确认后开始。")}</span>
        </div>
        <div className="scan-launcher-toolbar">
          {entries.map((entry) => (
            <button className={entry.label === "快速扫描" ? "pr" : "gh"} disabled={controlsDisabled} key={entry.label} onClick={entry.action}>
              <i className={`ti ${busy ? "ti-loader spin" : entry.icon}`} aria-hidden="true" />
              {tr(entry.label)}
            </button>
          ))}
        </div>
      </section>

      {selector && (
        <Modal
          title={tr(selector === "all" ? "全盘分析" : "选择磁盘")}
          icon="ti-device-hdd"
          sub={tr(selector === "all"
            ? "全盘分析不会预选磁盘。请选择一个或多个本地固定磁盘。"
            : "可恢复上次选择，但扫描不会自动开始。仅显示本地固定磁盘。")}
          onClose={() => !busy && setSelector(null)}
          footer={<>
            <button className="gh sm" disabled={busy} onClick={() => setSelector(null)}>{tr("取消")}</button>
            <button
              className="pr sm"
              disabled={busy || volumeLoading || selected.size === 0}
              onClick={() => launch({ mode: "drives", targets: [...selected] })}
            >
              <i className={`ti ${busy ? "ti-loader spin" : "ti-player-play"}`} />
              {busy ? tr("正在启动…") : tr("开始分析")}
            </button>
          </>}
        >
          <div className="scan-volume-list">
            {volumeLoading && (
              <div className="scan-volume-state"><i className="ti ti-loader spin" /> {tr("正在读取本地磁盘…")}</div>
            )}
            {!volumeLoading && volumeError && (
              <div className="scan-volume-state error"><i className="ti ti-alert-circle" /> {tr("无法读取本地磁盘，请关闭后重试。")}</div>
            )}
            {!volumeLoading && !volumeError && volumes.length === 0 && (
              <div className="scan-volume-state"><i className="ti ti-device-hdd-off" /> {tr("未发现可分析的本地固定磁盘。")}</div>
            )}
            {!volumeLoading && !volumeError && volumes.map((volume) => {
              const checked = selected.has(volume.root);
              const usedBytes = Math.max(0, volume.totalBytes - volume.freeBytes);
              return (
                <label className={`scan-volume-row${checked ? " selected" : ""}`} key={volume.root}>
                  <input
                    className="ck2"
                    type="checkbox"
                    checked={checked}
                    disabled={busy}
                    onChange={() => toggleVolume(volume.root)}
                  />
                  <span className="scan-volume-icon"><i className="ti ti-device-hdd" /></span>
                  <span className="scan-volume-main">
                    <strong>{volume.root}{volume.label ? ` ${volume.label}` : ""}</strong>
                    <span>{volume.fileSystem || tr("未知文件系统")} · {tr("已用")} {formatBytes(usedBytes)} / {formatBytes(volume.totalBytes)}</span>
                  </span>
                  <span className="scan-volume-free">{tr("可用")} {formatBytes(volume.freeBytes)}</span>
                </label>
              );
            })}
          </div>
          <div className="scan-selector-note">
            <i className="ti ti-shield-check" />
            <span>{tr("可移动磁盘、光驱和网络磁盘不会出现在此列表中。")}</span>
          </div>
        </Modal>
      )}
    </>
  );
}
