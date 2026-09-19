import { t } from "../../i18n";
import type { BridgeStatus } from "../../lib/bridgeMessages";

/** 「已连接 Stacker · 待同步 N 项」 or 「未连接 Stacker」, with the matching action. */
export function SyncStatus({ status, busy, onReconnect, onRestore }: {
  status: BridgeStatus | null; busy: boolean; onReconnect: () => void; onRestore: () => void;
}) {
  if (!status?.connected) {
    return <span className="mut" title={status?.error || undefined}>
      {t("未连接 Stacker")} <button disabled={busy} onClick={onReconnect}>{t("重新连接")}</button>
    </span>;
  }
  return <span className="mut">
    {t("已连接 Stacker")} · {t("待同步")} {status.pending} {t("项")} <button disabled={busy} onClick={onRestore}>{t("从 Stacker 恢复")}</button>
  </span>;
}
