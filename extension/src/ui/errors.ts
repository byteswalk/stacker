import { SiteError } from "../shared/types";
import { BridgeError } from "../lib/bridge";
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
  E_EMPTY: "正文为空，未删除",
  E_NOT_CONNECTED: "未连接 Stacker",
  E_TIMEOUT: "Stacker 没有响应，请稍后再试",
  E_PATH: "导出文件名无效",
  E_STORAGE: "Stacker 无法写入它的数据目录",
  E_REQUEST: "Stacker 拒绝了这个请求",
};

export function errorText(e: unknown): string {
  const code = e instanceof SiteError || e instanceof BridgeError ? e.code : typeof e === "string" ? e : "";
  return ERROR_TEXT[code] ? t(ERROR_TEXT[code]) : e instanceof Error ? e.message : String(e);
}
