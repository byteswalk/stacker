import type { Page } from "./pageState";

/** The page runs in Stacker's macOS build (WKWebView says "Macintosh"); Windows otherwise. */
export const IS_MAC = typeof navigator !== "undefined" && /Mac/i.test(navigator.userAgent);

/**
 * The pages only the Windows build has: runtime and toolchain management goes through the
 * Windows registry, WinGet and UAC, and Homebrew and mise already cover it on a Mac.
 */
const WINDOWS_ONLY: readonly Page[] = ["python", "php", "node", "java", "maven", "gradle", "go", "rust"];

export function pageAvailable(page: Page, mac = IS_MAC): boolean {
  return !mac || !WINDOWS_ONLY.includes(page);
}

/** What the system calls these, for text that names them. */
export const FILE_MANAGER = IS_MAC ? "访达" : "资源管理器";
export const TRASH = IS_MAC ? "废纸篓" : "回收站";
export const CREDENTIAL_STORE = IS_MAC ? "钥匙串" : "Windows 凭据管理器";
export const SYSTEM_NAME = IS_MAC ? "macOS" : "Windows";
export const TERMINAL = IS_MAC ? "终端" : "PowerShell";
