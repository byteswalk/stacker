import type { FindingStatus, Kind } from "./api";

export const KIND_ORDER: Kind[] = ["api_key", "token", "token_plan", "ak_sk", "ssh_key", "other"];

export const KIND_LABELS: Record<Kind, string> = {
  api_key: "API Key", token: "令牌", token_plan: "Token 计划", ak_sk: "AK/SK", ssh_key: "SSH 密钥", other: "其他",
};

export const TEMPLATES: Record<Kind, { name: string; secret: boolean }[]> = {
  api_key: [{ name: "Key", secret: true }, { name: "Base URL", secret: false }],
  token: [{ name: "Token", secret: true }, { name: "账号", secret: false }, { name: "权限范围", secret: false }],
  token_plan: [{ name: "Key", secret: true }, { name: "Base URL", secret: false }, { name: "套餐", secret: false }, { name: "额度/周期", secret: false }],
  ak_sk: [{ name: "Access Key ID", secret: false }, { name: "Secret Access Key", secret: true }],
  ssh_key: [{ name: "私钥", secret: true }, { name: "公钥", secret: false }, { name: "口令", secret: true }, { name: "用途/主机", secret: false }],
  other: [],
};

export const RISK_LABELS: Record<string, string> = {
  unencrypted: "私钥未加密", dsa: "算法过旧（DSA）", rsa_short: "RSA 长度不足 2048 位",
};

export const SOURCE_LABELS: Record<string, string> = {
  ssh: "SSH 密钥", config: "云与包管理配置", env: "环境变量", dotenv: "项目 .env",
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
