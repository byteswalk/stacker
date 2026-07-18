import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../../i18n";
import { invoke } from "../../../invoke";
import { Modal, operationWasCancelled, useToast } from "../../../ui";
import { scanSnapshotIsActive, startScan, useSpaceScan } from "../store";
import {
  loadRememberedTargets,
  rememberStartedScan,
} from "../targetStore";
import type { ScanRequest, VolumeInfo } from "../types";
import {
  beginDiskSelectorRequest,
  closeDiskSelectorRequest,
  createDiskSelectorState,
  diskSelectorResponseIsCurrent,
  launcherControlsDisabled,
  rememberSettingFrom,
  startAndRememberScan,
  type DiskSelectorKind,
  type DiskSelectorRequestIdentity,
  type DiskSelectorRow,
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

export function ScanLauncher({ disabled = false }: { disabled?: boolean }) {
  const { tr } = useI18n();
  const toast = useToast();
  const scan = useSpaceScan();
  const [rememberTargets, setRememberTargets] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [selector, setSelector] = useState<DiskSelectorKind | null>(null);
  const [rows, setRows] = useState<DiskSelectorRow[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [volumeLoading, setVolumeLoading] = useState(false);
  const [volumeError, setVolumeError] = useState(false);
  const selectorRequest = useRef<DiskSelectorRequestIdentity>({ generation: 0, kind: null });
  const controlsDisabled = launcherControlsDisabled({
    settings: rememberTargets,
    externallyDisabled: disabled,
    busy,
    scanActive: scanSnapshotIsActive(scan),
  });

  useEffect(() => {
    let current = true;
    invoke<SpaceAnalysisSettings>("settings_get")
      .then((settings) => {
        if (!current) return;
        const resolved = rememberSettingFrom(settings.remember_scan_targets);
        if (resolved === null) {
          throw new Error("invalid space-analysis settings");
        }
        setRememberTargets(resolved);
      })
      .catch(() => {
        if (current) toast(tr("无法读取空间分析设置。扫描入口已保持禁用，请重试。"), "err");
      });
    return () => {
      current = false;
    };
  }, [toast, tr]);

  useEffect(() => () => {
    selectorRequest.current = closeDiskSelectorRequest(selectorRequest.current.generation);
  }, []);

  function closeSelector() {
    selectorRequest.current = closeDiskSelectorRequest(selectorRequest.current.generation);
    setSelector(null);
    setRows([]);
    setSelected(new Set());
    setVolumeLoading(false);
    setVolumeError(false);
  }

  async function launch(request: ScanRequest) {
    setBusy(true);
    try {
      if (rememberTargets === null) return;
      const outcome = await startAndRememberScan(request, rememberTargets, {
        start: startScan,
        remember: (acceptedRequest, remember) => rememberStartedScan(
          acceptedRequest.mode,
          acceptedRequest.targets,
          remember,
        ),
      });
      if (outcome.memory && !outcome.memory.ok) {
        toast(tr("扫描已开始，但无法保存扫描目标。"), "info");
      }
      closeSelector();
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
    const request = beginDiskSelectorRequest(selectorRequest.current.generation, kind);
    selectorRequest.current = request;
    setSelector(kind);
    setRows([]);
    setSelected(new Set());
    setVolumeError(false);
    setVolumeLoading(true);
    try {
      const available = await invoke<VolumeInfo[]>("space_fixed_volumes");
      const remembered = kind === "drives" ? loadRememberedTargets("drives") : [];
      const state = createDiskSelectorState(kind, available, remembered);
      if (!diskSelectorResponseIsCurrent(selectorRequest.current, request)) return;
      setRows(state.rows);
      setSelected(new Set(state.selected));
    } catch {
      if (diskSelectorResponseIsCurrent(selectorRequest.current, request)) setVolumeError(true);
    } finally {
      if (diskSelectorResponseIsCurrent(selectorRequest.current, request)) setVolumeLoading(false);
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

  return (
    <>
      <section className="scan-launcher" aria-label={tr("选择扫描范围")}>
        <div className="scan-launcher-copy">
          <strong>{tr("开始空间分析")}</strong>
          <span>{tr("选择快速检查、目录或本地固定磁盘。扫描仅在手动确认后开始。")}</span>
        </div>
        <div className="scan-launcher-toolbar">
          <button className="pr" disabled={controlsDisabled} onClick={() => launch({ mode: "quick", targets: [] })}>
            <i className={`ti ${busy ? "ti-loader spin" : "ti-bolt"}`} aria-hidden="true" />
            {tr("快速扫描")}
          </button>
          <button className="gh" disabled={controlsDisabled} onClick={chooseFolder}>
            <i className={`ti ${busy ? "ti-loader spin" : "ti-folder-open"}`} aria-hidden="true" />
            {tr("选择目录")}
          </button>
          <button className="gh" disabled={controlsDisabled} onClick={() => openDiskSelector("drives")}>
            <i className={`ti ${busy ? "ti-loader spin" : "ti-device-hdd"}`} aria-hidden="true" />
            {tr("选择磁盘")}
          </button>
          <button className="gh" disabled={controlsDisabled} onClick={() => openDiskSelector("all")}>
            <i className={`ti ${busy ? "ti-loader spin" : "ti-chart-treemap"}`} aria-hidden="true" />
            {tr("全盘分析")}
          </button>
        </div>
      </section>

      {selector && (
        <Modal
          title={tr(selector === "all" ? "全盘分析" : "选择磁盘")}
          icon="ti-device-hdd"
          sub={tr(selector === "all"
            ? "全盘分析不会预选磁盘。请选择一个或多个本地固定磁盘。"
            : "可恢复上次选择，但扫描不会自动开始。仅显示本地固定磁盘。")}
          onClose={() => !busy && closeSelector()}
          footer={<>
            <button className="gh sm" disabled={busy} onClick={closeSelector}>{tr("取消")}</button>
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
            {!volumeLoading && !volumeError && rows.length === 0 && (
              <div className="scan-volume-state"><i className="ti ti-device-hdd-off" /> {tr("未发现可分析的本地固定磁盘。")}</div>
            )}
            {!volumeLoading && !volumeError && rows.map((row) => {
              const checked = row.available && selected.has(row.root);
              const usedBytes = Math.max(0, row.totalBytes - row.freeBytes);
              return (
                <label
                  className={`scan-volume-row${checked ? " selected" : ""}${row.available ? "" : " invalid"}`}
                  key={row.root}
                  aria-disabled={!row.available}
                >
                  <input
                    className="ck2"
                    type="checkbox"
                    checked={checked}
                    disabled={busy || !row.available}
                    onChange={() => row.available && toggleVolume(row.root)}
                  />
                  <span className="scan-volume-icon"><i className={`ti ${row.available ? "ti-device-hdd" : "ti-device-hdd-off"}`} /></span>
                  <span className="scan-volume-main">
                    <strong>{row.root}{row.label ? ` ${row.label}` : ""}</strong>
                    <span>{row.available
                      ? `${row.fileSystem || tr("未知文件系统")} · ${tr("已用")} ${formatBytes(usedBytes)} / ${formatBytes(row.totalBytes)}`
                      : tr("上次选择 · 当前不是可用的本地固定磁盘")}</span>
                  </span>
                  {row.available
                    ? <span className="scan-volume-free">{tr("可用")} {formatBytes(row.freeBytes)}</span>
                    : <span className="scan-volume-invalid">{tr("不可用")}</span>}
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
