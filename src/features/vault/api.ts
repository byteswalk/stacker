import { invoke } from "../../invoke";

export type Kind = "api_key" | "token" | "token_plan" | "ak_sk" | "ssh_key" | "other";
export type VaultState = "missing" | "locked" | "recovering" | "unlocked";
export type VaultStatus = { exists: boolean; state: VaultState; waitSeconds: number; pending: boolean };
export type SshInfo = { algorithm: string; bits: number | null; encrypted: boolean; fingerprint: string | null; publicKey: string | null; risks: string[] };
export type FieldView = { name: string; secret: boolean; value: string | null; filled: boolean };
export type EntryView = {
  id: string; title: string; platform: string; kind: Kind; fields: FieldView[]; expiresAt: string | null; tags: string[]; note: string;
  favorite: boolean; createdAt: number; updatedAt: number; deletedAt: number | null; historyCount: number; ssh: SshInfo | null;
};
export type FieldInput = { name: string; previousName: string | null; value: string | null; secret: boolean };
export type EntryInput = {
  id: string | null; title: string; platform: string; kind: Kind; fields: FieldInput[]; expiresAt: string | null; tags: string[]; note: string; favorite: boolean;
};
export type HistoryView = { index: number; field: string; at: number };
export type MergeStats = { added: number; updated: number; same: number };
export type Credential = { kind: "password" | "recovery"; value: string };
export type DiscoverScope = { ssh: boolean; configs: boolean; env: boolean; projectDirs: string[] };
export type FindingStatus = "new" | "in_vault" | "in_vault_old" | "ignored";
export type Finding = {
  id: number; source: "ssh" | "config" | "env" | "dotenv"; location: string; name: string; preview: string; platform: string; kind: Kind; risks: string[]; status: FindingStatus;
};
export type DiscoverStatus = { running: boolean; cancelled: boolean; truncated: boolean; files: number; findings: Finding[] };
export type SshKeyPair = { privateKey: string; publicKey: string };
export type SshLocal = { path: string | null; alias: string | null };
export type EnvHolder = { field: string; name: string; scope: "user" | "system" };
export type SshLocalHost = { alias: string; host: string; user: string; port: number };
export type RetiredVault = { path: string; modifiedMs: number; bytes: number };
export type ImportItem = { id: number; platform: string; kind: Kind; origin?: string };
export type VaultSettings = { vault_auto_lock_minutes: number; vault_scan_dirs: string[] };

export const vaultApi = {
  status: () => invoke<VaultStatus>("vault_status"),
  createBegin: (password: string) => invoke<string>("vault_create_begin", { password }),
  copyRecovery: () => invoke<void>("vault_copy_recovery"),
  confirmRecovery: (lastGroup: string) => invoke<void>("vault_confirm_recovery", { lastGroup }),
  cancelPending: () => invoke<void>("vault_cancel_pending"),
  unlock: (password: string) => invoke<void>("vault_unlock", { password }),
  unlockRecovery: (recoveryKey: string) => invoke<void>("vault_unlock_recovery", { recoveryKey }),
  recoverySetPassword: (password: string) => invoke<string>("vault_recovery_set_password", { password }),
  changePassword: (current: string, next: string) => invoke<void>("vault_change_password", { current, next }),
  resetRecovery: (password: string) => invoke<string>("vault_reset_recovery", { password }),
  lock: () => invoke<void>("vault_lock"),
  touch: () => invoke<void>("vault_touch"),
  list: (trash: boolean) => invoke<EntryView[]>("vault_list", { trash }),
  save: (input: EntryInput) => invoke<EntryView>("vault_save", { input }),
  reveal: (id: string, field: string) => invoke<string>("vault_reveal", { id, field }),
  copy: (id: string, field: string) => invoke<void>("vault_copy", { id, field }),
  history: (id: string) => invoke<HistoryView[]>("vault_history", { id }),
  historyReveal: (id: string, index: number) => invoke<string>("vault_history_reveal", { id, index }),
  historyCopy: (id: string, index: number) => invoke<void>("vault_history_copy", { id, index }),
  remove: (id: string) => invoke<void>("vault_delete", { id }),
  restore: (id: string) => invoke<void>("vault_restore", { id }),
  purge: (id: string) => invoke<void>("vault_purge", { id }),
  sshExport: (id: string, dest: string) => invoke<void>("vault_ssh_export", { id, dest }),
  sshGenerate: (algorithm: string, comment: string, passphrase: string) => invoke<SshKeyPair>("vault_ssh_generate", { algorithm, comment, passphrase }),
  sshSetPassphrase: (id: string, old: string, next: string, passphraseField: string) =>
    invoke<EntryView>("vault_ssh_set_passphrase", { id, old, new: next, passphraseField }),
  envHolders: (id: string) => invoke<EnvHolder[]>("vault_env_holders", { id }),
  sshLocal: (id: string) => invoke<SshLocal>("vault_ssh_local", { id }),
  sshInstallLocal: (id: string, name: string, host: SshLocalHost | null) => invoke<string>("vault_ssh_install_local", { id, name, host }),
  exportBackup: (password: string, dest: string) => invoke<void>("vault_export", { password, dest }),
  importPreview: (src: string, credential: Credential) => invoke<MergeStats>("vault_import_preview", { src, credential }),
  importApply: (src: string, credential: Credential) => invoke<MergeStats>("vault_import_apply", { src, credential }),
  restoreBackup: (src: string) => invoke<void>("vault_restore_backup", { src }),
  reset: () => invoke<string>("vault_reset"),
  retired: () => invoke<RetiredVault[]>("vault_retired"),
  clipboardText: () => invoke<string>("vault_clipboard_text"),
  discoverStart: (scope: DiscoverScope) => invoke<void>("vault_discover_start", { scope }),
  discoverStatus: () => invoke<DiscoverStatus>("vault_discover_status"),
  discoverCancel: () => invoke<void>("vault_discover_cancel"),
  discoverClear: () => invoke<void>("vault_discover_clear"),
  discoverImport: (items: ImportItem[], notePrefix: string) => invoke<number>("vault_discover_import", { items, notePrefix }),
  discoverIgnore: (ids: number[]) => invoke<void>("vault_discover_ignore", { ids }),
  settings: () => invoke<VaultSettings>("settings_get"),
  setSettings: (autoLockMinutes: number, scanDirs: string[]) => invoke<VaultSettings>("settings_set_vault", { autoLockMinutes, scanDirs }),
};

const ERRORS: Record<string, string> = {
  E_VAULT_MISSING: "尚未创建保管库。",
  E_VAULT_EXISTS: "本机已有保管库。",
  E_VAULT_LOCKED: "保管库已锁定，请先解锁。",
  E_VAULT_PASSWORD: "主密码不正确。",
  E_VAULT_RECOVERY: "恢复密钥不正确。",
  E_VAULT_WEAK: "主密码至少需要 9 个字符。",
  E_VAULT_WAIT: "尝试次数过多，请 30 秒后再试。",
  E_VAULT_CORRUPT: "文件已损坏，或不是有效的保管库文件。",
  E_VAULT_NEWER: "此保管库由更新版本的 Stacker 创建，请先升级。",
  E_VAULT_CHANGED: "保管库文件已在外部被修改，请重新解锁后再试。",
  E_VAULT_CONFIRM: "输入的字符与恢复密钥最后一组不一致。",
  E_VAULT_NO_PENDING: "操作已失效，请重新开始。",
  E_VAULT_NOT_FOUND: "条目不存在或已被删除。",
  E_VAULT_FILE_EXISTS: "目标位置已有同名文件，请选择其他位置。",
  E_VAULT_INVALID: "请填写标题。",
  E_VAULT_BUSY: "正在扫描，请等待完成或取消。",
  E_VAULT_PASSPHRASE: "当前口令不正确。",
  E_VAULT_KEY_FORMAT: "这把私钥不是 OpenSSH 格式，Stacker 改不了它的口令。",
  E_VAULT_NAME: "文件名只能用字母、数字、点、短横线和下划线，且不能是 config、known_hosts 这类 ssh 自己的文件。",
  E_VAULT_HOST: "别名、主机和用户只能用字母、数字、点、短横线和下划线。",
  E_VAULT_HOST_EXISTS: "~/.ssh/config 里已经有这个别名，请换一个。",
  E_VAULT_IO: "读写文件失败，请检查磁盘空间与权限后重试。",
};

export function vaultError(error: unknown): string {
  const code = String(error).replace(/^Error:\s*/, "").trim();
  return ERRORS[code] ?? code;
}
