/** Messages between the login content script and the background. */

export interface LoginChoice { user: string; title: string; host: string }

export type LoginMessage =
  | { type: "logins-page" }
  | { type: "logins-captured"; user: string; password: string; title: string }
  | { type: "logins-decide"; choice: "save" | "save-fill" | "never" | "dismiss" }
  | { type: "logins-fill"; user: string };

export interface PageAnswer {
  /** A login seen on this site a moment ago that is waiting for the user's answer. */
  pending: { user: string; host: string } | null;
  /** The logins Stacker can fill on this page. */
  logins: LoginChoice[];
}

const TYPES = new Set(["logins-page", "logins-captured", "logins-decide", "logins-fill"]);
const CHOICES = new Set(["save", "save-fill", "never", "dismiss"]);

/** Runtime shape check on an untrusted message before the background acts on it. */
export function isLoginMessage(message: unknown): message is LoginMessage {
  const m = message as Record<string, unknown> | null;
  if (!m || typeof m !== "object" || !TYPES.has(m.type as string)) return false;
  switch (m.type) {
    case "logins-captured":
      return typeof m.user === "string" && m.user.length <= 256 && typeof m.password === "string"
        && m.password.length > 0 && m.password.length <= 512 && typeof m.title === "string";
    case "logins-decide":
      return CHOICES.has(m.choice as string);
    case "logins-fill":
      return typeof m.user === "string" && m.user.length <= 256;
    default:
      return true;
  }
}

/** The site a page belongs to for logins: its host without `www.`, only for http(s) pages. */
export function siteOf(url: string | undefined): string | null {
  if (!url) return null;
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== "https:" && parsed.protocol !== "http:") return null;
    return parsed.hostname.replace(/^www\./, "").toLowerCase();
  } catch {
    return null;
  }
}

/** How long a submitted login waits for an answer, across the page the site sends you to. */
export const PENDING_MS = 3 * 60_000;
