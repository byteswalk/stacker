import { useEffect, useState } from "react";
import { invoke } from "../../invoke";
import { useToast } from "../../ui";
import { vaultApi, vaultError, type RetiredVault } from "./api";
import { formatTime } from "./vaultView";

const fileName = (path: string) => path.replace(/^.*[\\/]/, "");
const folderOf = (path: string) => path.replace(/[\\/][^\\/]+$/, "");

/** The vaults earlier resets set aside. Empty until the list is read, and when there are none. */
export function useRetiredVaults(): RetiredVault[] {
  const [retired, setRetired] = useState<RetiredVault[]>([]);
  useEffect(() => {
    let alive = true;
    vaultApi.retired().then((list) => { if (alive) setRetired(list ?? []); }).catch(() => undefined);
    return () => { alive = false; };
  }, []);
  return retired;
}

/** Where a reset put the old vault, and the ways back to it. `onPick` is what the row's button does. */
export function RetiredVaults({ retired, pickLabel, pickIcon, picked, onPick }: {
  retired: RetiredVault[]; pickLabel: string; pickIcon: string; picked?: string; onPick: (path: string) => void;
}) {
  const toast = useToast();
  if (!retired.length) return null;
  const open = () => void invoke("space_open_directory", { path: folderOf(retired[0].path) }).catch((error) => toast(vaultError(error), "err"));
  return (
    <div className="vault-retired">
      <div className="vault-retired-head">
        <span><i className="ti ti-archive" /> 重置时保留的旧保管库</span>
        <button type="button" className="gh xs" onClick={open}><i className="ti ti-folder-open" /> 打开所在位置</button>
      </div>
      <code className="vault-retired-dir" translate="no" title={folderOf(retired[0].path)}>{folderOf(retired[0].path)}</code>
      {retired.map((vault) => (
        <div className={"vault-retired-row" + (picked === vault.path ? " on" : "")} key={vault.path}>
          <code translate="no" title={vault.path}>{fileName(vault.path)}</code>
          <span className="mut">{formatTime(vault.modifiedMs)}</span>
          <button type="button" className="gh xs" onClick={() => onPick(vault.path)}><i className={"ti " + pickIcon} /> {pickLabel}</button>
        </div>
      ))}
    </div>
  );
}
