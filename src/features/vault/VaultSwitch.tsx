import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../i18n";
import { Modal, useToast } from "../../ui";
import { vaultApi, vaultError } from "./api";
import { RetiredVaults, useRetiredVaults } from "./RetiredVaults";

const fileName = (path: string) => path.replace(/^.*[\\/]/, "");

/**
 * Makes another vault file the current one: one set aside by a reset, or a backup picked from
 * disk. The current vault is kept under an `.old` name, so it appears in the same list and
 * switching back is the same step.
 */
export function VaultSwitch({ onClose, onSwitched }: { onClose: () => void; onSwitched: () => void }) {
  const { tr } = useI18n();
  const toast = useToast();
  const retired = useRetiredVaults();
  const [picked, setPicked] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function choose() {
    try {
      const path = await open({ title: tr("选择保管库文件"), multiple: false, directory: false, filters: [{ name: tr("Stacker 保管库"), extensions: ["skv", "old"] }] });
      if (typeof path === "string") setPicked(path);
    } catch (error) { toast(vaultError(error), "err"); }
  }

  async function confirm() {
    if (!picked) return;
    setBusy(true);
    try {
      const kept = await vaultApi.switchTo(picked);
      toast(kept
        ? tr("已切换。原来的保管库改名为 {name}，可随时切回").replace("{name}", fileName(kept))
        : tr("已切换，请用这个保管库自己的主密码或恢复密钥解锁"), "ok");
      onSwitched();
    } catch (error) {
      toast(vaultError(error), "err");
      setBusy(false);
    }
  }

  return (
    <Modal title="切换保管库" icon="ti-switch-horizontal" wide onClose={busy ? undefined : onClose}
      sub="选一个旧保管库或备份文件作为当前保管库。现在的保管库不会删除，会改名留在原文件夹并出现在下面的列表里，随时可以切回。切换后用选中保管库自己的主密码或恢复密钥解锁。"
      footer={<>
        <button className="gh sm" disabled={busy} onClick={onClose}>取消</button>
        <button className="pr sm" disabled={busy || !picked} onClick={() => void confirm()}>
          <i className={"ti " + (busy ? "ti-loader spin" : "ti-switch-horizontal")} /> {busy ? "切换中…" : "切换"}
        </button>
      </>}>
      <RetiredVaults retired={retired} pickLabel="选这个" pickIcon="ti-check" picked={picked ?? undefined} onPick={setPicked} />
      <div className="vault-switch-file">
        <button type="button" className="gh sm" disabled={busy} onClick={() => void choose()}><i className="ti ti-folder-open" /> 选择其他文件…</button>
        <span className="mut" translate="no" title={picked ?? undefined}>{picked ? fileName(picked) : tr("可选导出的加密备份（.skv）")}</span>
      </div>
    </Modal>
  );
}
