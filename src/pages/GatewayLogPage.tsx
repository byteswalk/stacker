import { useCallback, useEffect, useState } from "react";
import { invoke } from "../invoke";
import { useI18n } from "../i18n";
import { ErrorState, Loading, useToast } from "../ui";
import { GatewayLog } from "../features/gateway/GatewayLog";

type Status = { logEnabled: boolean; logRetentionDays: number };

/** The request log of the API service, as a page of its own rather than the bottom of one. */
export default function GatewayLogPage() {
  const { tr: t } = useI18n();
  const toast = useToast();
  const [status, setStatus] = useState<Status | null>(null);
  const [loadErr, setLoadErr] = useState(false);

  const load = useCallback(async () => {
    try {
      setStatus(await invoke<Status>("gateway_status"));
      setLoadErr(false);
    } catch {
      setLoadErr(true);
    }
  }, []);
  useEffect(() => { void load(); }, [load]);

  async function setLogPolicy(logEnabled: boolean, retentionDays: number) {
    try {
      await invoke("gateway_set_log", { enabled: logEnabled, retentionDays });
      await load();
    } catch (e) { toast(String(e), "err"); }
  }

  if (loadErr) return <ErrorState title={t("暂时无法读取接口服务状态")} description={t("请稍后重试。")} onRetry={load} />;
  if (!status) return <Loading text={t("正在读取接口日志…")} />;
  return <GatewayLog enabled={status.logEnabled} retentionDays={status.logRetentionDays}
    onSettings={(logEnabled, retentionDays) => void setLogPolicy(logEnabled, retentionDays)} />;
}
