import { useEffect, useState } from "react";
import { invoke } from "../invoke";
import { useToast, ConfirmModal, Loading, Modal, ErrorState } from "../ui";

type BackupEntry = { file: string; path: string; origin: string; time: string };
type BackupDetailItem = { label: string; value: string };
type BackupDetail = {
  kind: string;
  title: string;
  created: string;
  origin: string;
  backup_path: string;
  restore_note: string;
  items: BackupDetailItem[];
  preview?: string | null;
};

function prettyOrigin(origin: string) {
  const env = origin.match(/^env:\/\/(user|system)\/(.+)$/);
  if (env) return `${env[1] === "system" ? "系统级" : "用户级"}环境变量 · ${env[2]}`;
  const reg = origin.match(/^reg:\/\/hkcu\/(.+)$/);
  if (reg) return `注册表 · HKCU\\${reg[1].replace(/\//g, "\\")}`;
  return origin;
}

function kindLabel(kind: string) {
  if (kind === "env") return "环境变量";
  if (kind === "registry") return "注册表";
  return "文件";
}

export default function History() {
  const toast = useToast();
  const [items, setItems] = useState<BackupEntry[] | null>(null);
  const [loadErr, setLoadErr] = useState(false);
  const [confirm, setConfirm] = useState<BackupEntry | null>(null);
  const [detail, setDetail] = useState<BackupDetail | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<BackupEntry | null>(null);
  const [clearOpen, setClearOpen] = useState(false);
  const [busy, setBusy] = useState("");

  async function load() { setItems(await invoke<BackupEntry[]>("list_backups")); }
  useEffect(() => { load().catch(() => setLoadErr(true)); }, []);

  async function openDetail(item: BackupEntry) {
    setBusy("detail:" + item.path);
    try {
      setDetail(await invoke<BackupDetail>("backup_detail", { path: item.path, origin: item.origin }));
    } catch (e) {
      toast("读取备份详情失败：" + e, "err");
    } finally {
      setBusy("");
    }
  }

  async function doRestore() {
    if (!confirm) return;
    setBusy("restore");
    try {
      await invoke("restore_backup", { path: confirm.path, origin: confirm.origin });
      toast("已还原 " + confirm.file, "ok");
      setConfirm(null);
      await load();
    } catch (e) { toast("还原失败：" + e, "err"); } finally { setBusy(""); }
  }

  async function doDelete() {
    if (!deleteTarget) return;
    setBusy("delete");
    try {
      await invoke("delete_backup", { path: deleteTarget.path });
      toast("备份记录已删除", "ok");
      setDeleteTarget(null);
      await load();
    } catch (e) {
      toast("删除备份失败：" + e, "err");
    } finally {
      setBusy("");
    }
  }

  async function doClear() {
    setBusy("clear");
    try {
      const n = await invoke<number>("clear_backups");
      toast(`已清除 ${n} 条备份记录`, "ok");
      setClearOpen(false);
      await load();
    } catch (e) {
      toast("清除备份失败：" + e, "err");
    } finally {
      setBusy("");
    }
  }

  if (loadErr) return <ErrorState title="暂时无法读取备份记录" description="请确认 Stacker 配置目录可访问，然后重试。" onRetry={async () => { await load(); setLoadErr(false); }} />;
  const historyLoading = !items;
  const rows = items ?? [];

  return (
    <>
      <div className="grouphd">
        <span className="gt">备份记录 <span className="cnt">{historyLoading ? "读取中" : `${rows.length} 条`}</span></span>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span className="hint2">修改镜像、环境变量或终端集成前会自动备份，可随时还原</span>
          {rows.length > 0 && <button className="gh sm" onClick={() => setClearOpen(true)}><i className="ti ti-trash-x" /> 全部清除</button>}
        </div>
      </div>
      {historyLoading && <Loading text="正在读取备份记录…" />}
      {!historyLoading && rows.length === 0 && (
        <div className="stub"><div className="si"><i className="ti ti-history" /></div><h2>暂无备份</h2>
          <p>配置过镜像或修改过环境后，这里会自动出现可还原的备份。</p></div>
      )}
      {rows.map((h) => (
        <div className="srcrow" key={h.path}>
          <span className="av file"><i className="ti ti-file" /></span>
          <div className="mt"><div className="t">{h.file}</div><div className="s mono">{h.time} · {prettyOrigin(h.origin)}</div></div>
          <button className="gh sm" disabled={busy === "detail:" + h.path} onClick={() => openDetail(h)}>
            <i className={"ti " + (busy === "detail:" + h.path ? "ti-loader spin" : "ti-info-circle")} /> 详情
          </button>
          <button className="gh sm" onClick={() => setConfirm(h)}><i className="ti ti-arrow-back-up" /> 还原</button>
          <button className="gh sm" onClick={() => setDeleteTarget(h)}><i className="ti ti-trash" /> 删除</button>
        </div>
      ))}

      {detail && (
        <Modal wide title="备份详情" icon="ti-info-circle" onClose={() => setDetail(null)}
          sub={<span>{kindLabel(detail.kind)} · {detail.created || "未知时间"}</span>}
          footer={<button className="pr sm" onClick={() => setDetail(null)}>关闭</button>}>
          <div className="srcrow" style={{ alignItems: "flex-start" }}>
            <span className="av file"><i className="ti ti-restore" /></span>
            <div className="mt">
              <div className="t">{detail.title}</div>
              <div className="s dim">{detail.restore_note}</div>
              <div className="s mono" title={detail.origin}>{prettyOrigin(detail.origin)}</div>
            </div>
          </div>
          <div className="histdetail">
            {detail.items.map((it) => (
              <div className="histkv" key={it.label}>
                <span>{it.label}</span>
                <b title={it.value}>{it.value}</b>
              </div>
            ))}
          </div>
          {detail.preview && (
            <div>
              <div className="grouphd" style={{ marginTop: 0 }}><span className="gt">备份内容预览</span></div>
              <pre className="console" style={{ whiteSpace: "pre-wrap", maxHeight: 260, overflow: "auto" }}>{detail.preview}</pre>
            </div>
          )}
        </Modal>
      )}

      {confirm && (
        <ConfirmModal title={"还原 " + confirm.file} icon="ti-arrow-back-up"
          message={<>将用此备份还原 <b style={{ color: "var(--tx)" }}>{prettyOrigin(confirm.origin)}</b>。还原前会先备份当前状态，可再次回退。</>}
          confirmLabel={busy === "restore" ? "还原中…" : "还原"} busy={busy === "restore"}
          onConfirm={doRestore} onClose={() => setConfirm(null)} />
      )}

      {deleteTarget && (
        <ConfirmModal title="删除备份记录" icon="ti-trash" danger busy={busy === "delete"}
          message={<>确定删除 <b style={{ color: "var(--tx)" }}>{deleteTarget.file}</b>？删除后不能再用这条记录还原。</>}
          confirmLabel={busy === "delete" ? "删除中…" : "删除"}
          onConfirm={doDelete} onClose={() => setDeleteTarget(null)} />
      )}

      {clearOpen && (
        <ConfirmModal title="清除全部备份" icon="ti-trash-x" danger busy={busy === "clear"}
          message={<>确定清除全部 <b style={{ color: "var(--tx)" }}>{rows.length}</b> 条备份记录？清除后不能通过历史页还原这些状态。</>}
          confirmLabel={busy === "clear" ? "清除中…" : "全部清除"}
          onConfirm={doClear} onClose={() => setClearOpen(false)} />
      )}
    </>
  );
}
