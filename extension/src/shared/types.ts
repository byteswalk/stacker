export type SiteId = "chatgpt" | "claude" | "gemini" | "grok" | "deepseek";
export type Role = "user" | "assistant" | "system" | "tool";

export interface RemoteAccount { remoteId: string; label: string }
export interface RemoteConversation { id: string; title: string; createdAt: number; updatedAt: number; archived: boolean }
export interface Message { role: Role; text: string; at: number | null; attachments: string[] }
export interface RemoteBody { id: string; title: string; updatedAt: number; messages: Message[] }
export interface ListPage { items: RemoteConversation[]; next: string | null }

export type ErrorCode =
  | "E_BROKEN" | "E_RATE" | "E_AUTH" | "E_NOT_FOUND" | "E_HTTP" | "E_NET"
  | "E_NO_TAB" | "E_NO_AGENT" | "E_ACCOUNT" | "E_CANCELLED" | "E_EMPTY";

export class SiteError extends Error {
  code: ErrorCode;
  detail: string;
  retryAfterMs: number | null;

  constructor(code: ErrorCode, detail = "", retryAfterMs: number | null = null) {
    super(`${code}${detail ? `: ${detail}` : ""}`);
    this.code = code;
    this.detail = detail;
    this.retryAfterMs = retryAfterMs;
  }
}

export interface SerializedError { code: ErrorCode; detail: string; retryAfterMs: number | null }

export function serializeError(e: unknown): SerializedError {
  if (e instanceof SiteError) return { code: e.code, detail: e.detail, retryAfterMs: e.retryAfterMs };
  return { code: "E_NET", detail: e instanceof Error ? e.message : String(e), retryAfterMs: null };
}

export function reviveError(e: SerializedError): SiteError {
  return new SiteError(e.code, e.detail, e.retryAfterMs);
}
