import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "./invoke";
import { ConfirmModal, Modal, operationWasCancelled, useBusy, useBusyRead, useToast } from "./ui";

type StorageLocation = {
  id: string;
  ecosystem: string;
  label: string;
  description: string;
  path: string;
  default_path: string;
  source: "tool_default" | "user_config" | "env" | "system_env";
  size_bytes: number;
  advanced: boolean;
};

function formatBytes(value: number) {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = value;
  let index = 0;
  while (size >= 1024 && index < units.length - 1) {
    size /= 1024;
    index += 1;
  }
  return `${size >= 10 || index === 0 ? size.toFixed(0) : size.toFixed(1)} ${units[index]}`;
}

function sourceLabel(source: StorageLocation["source"]) {
  if (source === "env") return "环境变量";
  if (source === "system_env") return "系统环境变量";
  if (source === "user_config") return "用户配置";
  return "工具默认";
}

export function StorageLocations({ ecosystem }: { ecosystem: string }) {
  const toast = useToast();
  const runBusy = useBusy();
  const read = useBusyRead();
  const [rows, setRows] = useState<StorageLocation[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [change, setChange] = useState<{ row: StorageLocation; path: string } | null>(null);
  const [migrate, setMigrate] = useState(true);
  const [reset, setReset] = useState<StorageLocation | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError("");
    try {
      setRows(await read("正在读取存储配置", () => invoke<StorageLocation[]>("storage_locations", { ecosystem })));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setLoading(false);
    }
  }, [ecosystem, read]);

  useEffect(() => { void load(); }, [load]);

  const choose = async (row: StorageLocation) => {
    try {
      const selected = await open({ directory: true, multiple: false, defaultPath: row.path });
      if (typeof selected !== "string") return;
      setMigrate(true);
      setChange({ row, path: selected });
    } catch (cause) {
      toast(`无法选择目录：${cause}`, "err");
    }
  };

  const apply = async () => {
    if (!change) return;
    const { row, path } = change;
    try {
      await runBusy({
        title: `更改 ${row.label}`,
        message: migrate
          ? "正在复制现有内容并写入新配置。旧目录会保留，确认新位置可用后可自行清理。"
          : "正在写入新位置。现有内容不会移动或删除。",
        progressEvent: "storage-progress",
        cancel: {
          label: "取消更改",
          onCancel: () => { invoke("op_cancel").catch(() => undefined); },
        },
      }, async () => {
        const updated = await invoke<StorageLocation>("storage_apply", { id: row.id, path, migrate });
        setRows((current) => current.map((item) => item.id === updated.id ? updated : item));
        setChange(null);
      });
      toast(`${row.label} 已更新，新终端或下次运行工具时生效`, "ok");
    } catch (cause) {
      if (operationWasCancelled(cause)) {
        toast("已取消更改存储位置", "info");
        return;
      }
      toast(`更改 ${row.label} 失败：${cause}`, "err");
    }
  };

  const resetLocation = async () => {
    if (!reset) return;
    const row = reset;
    try {
      await runBusy({
        title: `恢复 ${row.label}`,
        message: "正在恢复工具默认位置。当前目录及其中内容不会被删除。",
      }, async () => {
        const updated = await invoke<StorageLocation>("storage_reset", { id: row.id });
        setRows((current) => current.map((item) => item.id === updated.id ? updated : item));
        setReset(null);
      });
      toast(`${row.label} 已恢复默认位置`, "ok");
    } catch (cause) {
      toast(`恢复 ${row.label} 失败：${cause}`, "err");
    }
  };

  const openDirectory = async (path: string) => {
    try {
      await invoke("space_open_directory", { path });
    } catch (cause) {
      toast(`无法打开目录。请确认路径存在且当前账号有访问权限。原因：${cause}`, "err");
    }
  };

  return (
    <>
      <section className="storage-section">
        <div className="grouphd storage-heading">
          <span className="gt"><i className="ti ti-database-cog" /> 存储位置</span>
          <span className="hint2">管理依赖、下载与构建缓存的默认目录；更改前会自动备份配置</span>
        </div>
        <div className="storage-panel">
          {loading ? (
            <div className="storage-state"><i className="ti ti-loader spin" /> 正在读取存储配置…</div>
          ) : error ? (
            <div className="storage-state error"><i className="ti ti-alert-circle" /> 读取失败：{error}<button className="gh xs" onClick={() => void load()}>重试</button></div>
          ) : (
            <div className="storage-list">
            {rows.map((row) => {
              const customized = row.source === "env" || row.source === "user_config";
              return (
                <div className="storage-row" key={row.id}>
                  <span className="av"><i className="ti ti-folder-cog" /></span>
                  <div className="storage-copy">
                    <div className="t" title={`${row.label}：${row.description}`}>
                      {row.label} <span className={`bd ${customized ? "g" : "n"}`}>{sourceLabel(row.source)}</span>
                      {row.advanced && <span className="bd y" title="此位置同时影响命令入口或用户 PATH，请仅在明确需要时更改">高级</span>}
                    </div>
                    <div className="s dim storage-description" title={row.description}>{row.description}</div>
                    <div className="s mono storage-path" title={row.path}>{row.path}</div>
                  </div>
                  {row.size_bytes > 0 && <span className="storage-size" title="当前目录占用">{formatBytes(row.size_bytes)}</span>}
                  <div className="storage-actions">
                    <button className="gh xs" title="在文件资源管理器中打开" onClick={() => void openDirectory(row.path)}><i className="ti ti-folder-open" /> 打开</button>
                    <button className="pr sm" title={`选择新的 ${row.label}`} onClick={() => void choose(row)}><i className="ti ti-folder-plus" /> 更改</button>
                    <button className="gh xs" disabled={!customized} title={customized ? "恢复默认位置，不删除当前目录" : row.source === "system_env" ? "当前继承系统配置，可更改为用户配置" : "当前已使用工具默认位置"} onClick={() => setReset(row)}><i className="ti ti-restore" /> 恢复默认</button>
                  </div>
                </div>
              );
            })}
            </div>
          )}
        </div>
      </section>

      {change && (
        <Modal title={`更改 ${change.row.label}`} icon="ti-folder-cog" onClose={() => setChange(null)} footer={<>
          <button className="gh sm" onClick={() => setChange(null)}>取消</button>
          <button className="pr sm" onClick={() => void apply()}><i className="ti ti-check" /> 确认更改</button>
        </>}>
          <div className="storage-change-path"><span>新位置</span><code title={change.path}>{change.path}</code></div>
          <label className="storage-migrate-option">
            <input type="checkbox" checked={migrate} onChange={(event) => setMigrate(event.target.checked)} />
            <span><b>复制现有内容到新位置</b><small>请先停止使用该工具，并选择空目录。取消或失败时配置不变，已复制的文件可能保留在新目录。旧目录始终保留，不会自动删除。</small></span>
          </label>
          <div className="callout"><i className="ti ti-info-circle" /><div>{change.row.advanced
            ? "这是高级位置。更改后会同步当前用户的命令入口或 PATH，新打开的终端生效。"
            : "更改只影响此工具后续使用的存储目录，不会修改项目文件。"}</div></div>
        </Modal>
      )}

      {reset && <ConfirmModal
        title={`恢复 ${reset.label}`}
        icon="ti-restore"
        message={<>将恢复到工具默认位置：<span className="code">{reset.default_path}</span>。当前目录及其中内容不会被删除。</>}
        confirmLabel="恢复默认"
        onClose={() => setReset(null)}
        onConfirm={() => void resetLocation()}
      />}
    </>
  );
}
