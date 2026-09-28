import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../../i18n";
import { DiskOverview } from "./DiskOverview";
import { invoke } from "../../../invoke";
import { Modal, operationWasCancelled, useToast } from "../../../ui";
import { scanSnapshotIsActive, startScan, useSpaceScan } from "../store";
import {
  loadRememberedTargets,
  rememberStartedScan,
  takePendingDirectoryTargets,
} from "../targetStore";
import type { ScanRequest } from "../types";
import {
  closeDiskSelectorRequest,
  launcherControlsDisabled,
  nonOverlappingDirectoryTargets,
  rememberSettingFrom,
  startAndRememberScan,
  type DiskSelectorRequestIdentity,
} from "../launcherViewModel";

type SpaceAnalysisSettings = {
  remember_scan_targets?: boolean;
  common_scan_directories?: string[];
};


export function ScanLauncher({ disabled = false }: { disabled?: boolean }) {
  const { tr } = useI18n();
  const toast = useToast();
  const scan = useSpaceScan();
  const [rememberTargets, setRememberTargets] = useState<boolean | null>(null);
  const [commonDirectories, setCommonDirectories] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [directorySelectorOpen, setDirectorySelectorOpen] = useState(false);
  const [directoryTargets, setDirectoryTargets] = useState<string[]>([]);
  const [elevated, setElevated] = useState(false);
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
        setCommonDirectories((settings.common_scan_directories ?? []).filter((path) => path.trim().length > 0));
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

  useEffect(() => {
    const pending = nonOverlappingDirectoryTargets(takePendingDirectoryTargets());
    if (pending.length === 0) return;
    setDirectoryTargets(pending);
    setElevated(false);
    setDirectorySelectorOpen(true);
  }, []);

  function closeSelector() {
    selectorRequest.current = closeDiskSelectorRequest(selectorRequest.current.generation);
    setDirectorySelectorOpen(false);
    setDirectoryTargets([]);
    setElevated(false);
  }

  async function launch(request: ScanRequest, useElevation = false) {
    setBusy(true);
    try {
      if (rememberTargets === null) return;
      const outcome = await startAndRememberScan(request, rememberTargets, {
        start: (acceptedRequest) => startScan(acceptedRequest, { elevated: useElevation }),
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
    } catch (error) {
      const message = String(error);
      if (useElevation && message.toLowerCase().includes("cancel")) {
        toast(tr("已取消管理员授权，扫描未开始。"), "info");
      } else {
        toast(tr("无法启动扫描，请重试。"), "err");
      }
    } finally {
      setBusy(false);
    }
  }

  function openDirectorySelector() {
    setDirectoryTargets(nonOverlappingDirectoryTargets(loadRememberedTargets("directories")));
    setElevated(false);
    setDirectorySelectorOpen(true);
  }

  async function addDirectory() {
    try {
      const chosen = await open({
        directory: true,
        multiple: false,
        defaultPath: directoryTargets[0],
        title: tr("选择要分析的目录"),
      });
      if (typeof chosen === "string" && chosen.length > 0) {
        setDirectoryTargets((current) => nonOverlappingDirectoryTargets([...current, chosen]));
      }
    } catch (error) {
      if (!operationWasCancelled(error)) toast(tr("无法打开目录选择器，请重试。"), "err");
    }
  }





  return (
    <>
      <section className="scan-launcher" aria-label={tr("选择扫描范围")}>
        <div className="scan-launcher-copy">
          <strong>{tr("开始空间分析")}</strong>
          <span>{tr("先看本机磁盘，再决定扫哪里：快速扫描只看已知的缓存与临时目录，选择目录可以深入到任意范围（含磁盘根目录）。")}</span>
        </div>
        <DiskOverview disabled={controlsDisabled} onScan={(root) => launch({ mode: "directories", targets: [root] })} />
        <div className="scan-launcher-toolbar">
          <button className="pr" disabled={controlsDisabled} title={tr("扫描常见开发缓存、历史版本和 Windows 临时目录，不会遍历整个磁盘。")} onClick={() => launch({ mode: "quick", targets: [] })}>
            <i className={`ti ${busy ? "ti-loader spin" : "ti-bolt"}`} aria-hidden="true" />
            {tr("快速扫描")}
          </button>
          <button className="gh" disabled={controlsDisabled} title={tr("选择一个或多个目录深入分析；选磁盘根目录就是全盘分析。")} onClick={openDirectorySelector}>
            <i className={`ti ${busy ? "ti-loader spin" : "ti-folder-open"}`} aria-hidden="true" />
            {tr("选择目录")}
          </button>
        </div>
        {commonDirectories.length > 0 && (
          <div className="scan-common-directories" aria-label={tr("常用扫描目录")}>
            <span>{tr("常用目录")}</span>
            {commonDirectories.map((path) => (
              <button className="gh sm" disabled={controlsDisabled} key={path} title={path} onClick={() => launch({ mode: "directories", targets: [path] })}>
                <i className="ti ti-folder" aria-hidden="true" />
                <span>{path}</span>
              </button>
            ))}
          </div>
        )}
      </section>

      {directorySelectorOpen && (
        <Modal
          title={tr("选择分析目录")}
          icon="ti-folders"
          sub={tr("可连续添加多个目录；存在包含关系时仅保留上层目录，避免重复统计。")}
          onClose={() => !busy && closeSelector()}
          footer={<>
            <button className="gh sm" disabled={busy} onClick={closeSelector}>{tr("取消")}</button>
            <button
              className="pr sm"
              disabled={busy || directoryTargets.length === 0}
              onClick={() => launch({ mode: "directories", targets: directoryTargets }, elevated)}
            >
              <i className={`ti ${busy ? "ti-loader spin" : "ti-player-play"}`} />
              {busy ? tr("正在启动…") : tr("开始分析")}
            </button>
          </>}
        >
          <label className="scan-elevation-option">
            <input className="ck2" type="checkbox" checked={elevated} disabled={busy} onChange={(event) => setElevated(event.target.checked)} />
            <span>
              <strong>{tr("使用管理员权限扫描")}</strong>
              <small>{tr("适用于包含受保护目录的范围；开始时将显示 Windows 用户账户控制提示。")}</small>
            </span>
          </label>
          <div className="scan-directory-toolbar">
            <button className="gh sm" disabled={busy} onClick={addDirectory}>
              <i className="ti ti-folder-plus" /> {tr("添加目录")}
            </button>
            {directoryTargets.length > 0 && (
              <span>{tr("已选择 {count} 个目录").replace("{count}", String(directoryTargets.length))}</span>
            )}
          </div>
          <div className="scan-directory-list">
            {directoryTargets.length === 0 && (
              <div className="scan-volume-state">
                <i className="ti ti-folders" /> {tr("尚未选择目录，请点击“添加目录”。")}
              </div>
            )}
            {directoryTargets.map((path) => (
              <div className="scan-directory-row" key={path} title={path}>
                <span className="scan-volume-icon"><i className="ti ti-folder" /></span>
                <span>{path}</span>
                <button
                  className="ic danger"
                  disabled={busy}
                  title={tr("移除目录")}
                  aria-label={tr("移除目录")}
                  onClick={() => setDirectoryTargets((current) => current.filter((target) => target !== path))}
                >
                  <i className="ti ti-trash" />
                </button>
              </div>
            ))}
          </div>
        </Modal>
      )}

    </>
  );
}
