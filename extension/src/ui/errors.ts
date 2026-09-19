import { SiteError } from "../shared/types";
import { t } from "../i18n";

export const ERROR_TEXT: Record<string, string> = {
  E_NO_TAB: "请先在浏览器中打开并登录该网站",
  E_NO_AGENT: "请刷新该网站页面后重试",
  E_AUTH: "该网站未登录或登录已过期",
  E_BROKEN: "该网站接口已变化，已停止操作，等待插件更新",
  E_RATE: "请求过于频繁，稍后再试",
  E_ACCOUNT: "不是当前登录的账号，已跳过",
  E_CANCELLED: "已中止",
  E_NOT_FOUND: "对话不存在",
  E_HTTP: "网络或网站错误",
  E_NET: "网络或网站错误",
};

export function errorText(e: unknown): string {
  const code = e instanceof SiteError ? e.code : typeof e === "string" ? e : "";
  return ERROR_TEXT[code] ? t(ERROR_TEXT[code]) : e instanceof Error ? e.message : String(e);
}
