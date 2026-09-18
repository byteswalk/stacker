import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { Select } from "../../Select";
import { Modal } from "../../ui";
import { errorMessage } from "./errors";
import type { Settings, Source, SourceKind } from "./types";

export function SourceSettings({ settings, onClose, onSaved }: { settings: Settings; onClose: () => void; onSaved: () => void }) {
  const { tr: t } = useI18n();
  const [sources, setSources] = useState(settings.sources);
  const [endpoint, setEndpoint] = useState(settings.model.endpoint);
  const [model, setModel] = useState(settings.model.model);
  const [key, setKey] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  function update(id: string, patch: Partial<Source>) { setSources((all) => all.map((s) => s.id === id ? { ...s, ...patch } : s)); }
  async function choose(id?: string) {
    try {
      const path = await open({ directory: true, multiple: false });
      if (typeof path !== "string") return;
      if (id) update(id, { root: path });
      else setSources((all) => [...all, { id: crypto.randomUUID(), name: "Import", kind: "import", root: path, enabled: true }]);
    } catch (e) { setError(errorMessage(e)); }
  }
  async function save() {
    setBusy(true); setError("");
    try {
      await invoke("conversations_save_settings", { items: sources, endpoint, model, key });
      onSaved(); onClose();
    } catch (e) { setError(errorMessage(e)); } finally { setBusy(false); }
  }
  return <Modal title={t("数据来源与总结模型")} wide onClose={busy ? undefined : onClose} footer={<>
    <button className="gh sm" onClick={onClose} disabled={busy}>{t("取消")}</button>
    <button className="pr sm" onClick={() => void save()} disabled={busy}><i className="ti ti-device-floppy" />{t("保存")}</button>
  </>}>
    <div className="conversation-settings">
      <div className="conversation-heading"><b>{t("本机会话来源")}</b><button className="gh sm" onClick={() => void choose()}><i className="ti ti-folder-plus" />{t("添加目录")}</button></div>
      {sources.map((source) => <div className="conversation-source" key={source.id}>
        <input type="checkbox" checked={source.enabled} aria-label={`${t("启用")} ${source.name}`} onChange={(e) => update(source.id, { enabled: e.target.checked })} />
        <input className="ip full" value={source.name} aria-label={t("来源名称")} onChange={(e) => update(source.id, { name: e.target.value })} />
        <Select value={source.kind} onChange={(kind) => update(source.id, { kind: kind as SourceKind })} options={[
          { value: "codex", label: "Codex" }, { value: "claude_cli", label: "Claude Code CLI" },
          { value: "claude_desktop", label: "Claude Desktop JSONL" }, { value: "import", label: t("Stacker 导出资料") },
        ]} />
        <button className="gh sm" title={source.root} onClick={() => void choose(source.id)}><i className="ti ti-folder" /><span>{source.root}</span></button>
        <button className="gh sm" title={t("移除数据来源，不删除原文件")} aria-label={t("移除数据来源，不删除原文件")} onClick={() => setSources((all) => all.filter((s) => s.id !== source.id))}><i className="ti ti-x" /></button>
      </div>)}
      <p className="conversation-note">{t("仅包含本机可读记录，不代表账号全部历史。Claude 桌面端与 CLI 分开接入；未知格式不会修改。")}</p>
      <h3>{t("总结模型")}</h3>
      <label>{t("兼容 Chat Completions 的完整地址")}<input className="ip full" value={endpoint} placeholder="http://localhost:11434/v1/chat/completions" onChange={(e) => setEndpoint(e.target.value)} /></label>
      <label>{t("模型名称")}<input className="ip full" value={model} onChange={(e) => setModel(e.target.value)} /></label>
      <label>API Key<input className="ip full" type="password" autoComplete="new-password" value={key ?? ""} placeholder={settings.model.has_key ? t("已加密保存，留空保持不变") : t("本机模型可留空")} onChange={(e) => setKey(e.target.value)} /></label>
      {settings.model.has_key && <button className="gh sm" onClick={() => setKey("")}>{t("清除已保存密钥")}</button>}
      <p className="conversation-note">{t("总结前会预览实际发送内容。聊天可能包含代码、路径或秘密；自动脱敏不能保证覆盖全部敏感信息。")}</p>
      <label>{t("本地索引与备份")}<code>{settings.storage}</code></label>
      {error && <p role="alert" className="conversation-error">{t(error)}</p>}
    </div>
  </Modal>;
}
