import { CloudDownloadOutlined, DisconnectOutlined } from "@ant-design/icons";
import { Badge, Button, Space, Tooltip } from "antd";
import { t } from "../../i18n";
import type { BridgeStatus } from "../../lib/bridgeMessages";

/** 「已连接 Stacker · 待同步 N 项」 or 「未连接 Stacker」, with the matching action. */
export function SyncStatus({ status, busy, onReconnect, onRestore }: {
  status: BridgeStatus | null; busy: boolean; onReconnect: () => void; onRestore: () => void;
}) {
  if (!status?.connected) {
    return <Space size={4}>
      <Tooltip title={status?.error || undefined}>
        <span><Badge status="default" text={t("未连接 Stacker")} /></span>
      </Tooltip>
      <Button size="small" type="text" icon={<DisconnectOutlined />} disabled={busy} onClick={onReconnect}>{t("重新连接")}</Button>
    </Space>;
  }
  const text = `${t("已连接 Stacker")} · ${t("待同步")} ${status.pending} ${t("项")}`;
  return <Space size={4}>
    <Tooltip title={status.lastSyncAt ? `${t("最近同步")} ${new Date(status.lastSyncAt).toLocaleString()}` : undefined}>
      <span><Badge status="success" text={text} /></span>
    </Tooltip>
    <Tooltip title={t("从 Stacker 恢复账号备注名、文件夹、标签、收藏、备注和摘录")}>
      <Button size="small" type="text" icon={<CloudDownloadOutlined />} disabled={busy} onClick={onRestore}>{t("从 Stacker 恢复")}</Button>
    </Tooltip>
  </Space>;
}
