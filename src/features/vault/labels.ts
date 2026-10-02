import type { FindingStatus, Kind } from "./api";

/**
 * Two kinds: an SSH key, which the vault can generate and hand to ssh, and a general
 * credential for everything else, whose fields the user names. Older kinds still read from
 * earlier vaults are shown as the general one.
 */
export const KIND_ORDER: Kind[] = ["other", "ssh_key"];

const GENERAL = "通用凭据";
export const KIND_LABELS: Record<Kind, string> = {
  other: GENERAL, api_key: GENERAL, token: GENERAL, token_plan: GENERAL, ak_sk: GENERAL, ssh_key: "SSH 密钥",
};

/** The field a general credential starts with; the user renames it or adds more. */
const GENERAL_FIELDS = [{ name: "密钥", secret: true }];
export const TEMPLATES: Record<Kind, { name: string; secret: boolean }[]> = {
  other: GENERAL_FIELDS, api_key: GENERAL_FIELDS, token: GENERAL_FIELDS, token_plan: GENERAL_FIELDS, ak_sk: GENERAL_FIELDS,
  ssh_key: [{ name: "私钥", secret: true }, { name: "公钥", secret: false }, { name: "口令", secret: true }],
};

/** `ssh_key` as it is; any other kind is the general credential. */
export const generalKind = (kind: Kind): Kind => kind === "ssh_key" ? "ssh_key" : "other";

export const RISK_LABELS: Record<string, string> = {
  unencrypted: "私钥未加密", dsa: "算法过旧（DSA）", rsa_short: "RSA 长度不足 2048 位",
};

export const SOURCE_LABELS: Record<string, string> = {
  ssh: "SSH 密钥", config: "云与包管理配置", env: "环境变量", credential: "Windows 凭据管理器（Git）", dotenv: "项目 .env",
};

export const ENV_SCOPE_LABELS: Record<string, string> = { user: "用户环境变量", system: "系统环境变量" };

export const STATUS_LABELS: Record<FindingStatus, string> = {
  new: "新发现", in_vault: "已在保管库", in_vault_old: "已在保管库（旧值）", ignored: "已忽略",
};

export const AUTO_LOCK_CHOICES = [5, 10, 30, 60];

export function lockReasonText(reason: string, minutes: number): string {
  if (reason === "session") return "Windows 已锁屏，保管库已锁定。";
  if (reason === "sleep") return "系统曾进入睡眠，保管库已锁定。";
  return `空闲 ${minutes} 分钟，保管库已锁定。`;
}
