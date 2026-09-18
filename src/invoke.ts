import { invoke as tauriInvoke, type InvokeArgs, type InvokeOptions } from "@tauri-apps/api/core";
import { debug, error, warn } from "@tauri-apps/plugin-log";

const quietCommands = new Set(["settings_read_log"]);

export function safeLogText(value: unknown): string {
  return String(value)
    .replace(/\bpt-[A-Za-z0-9_-]{16,}\b/g, "[REDACTED]")
    .replace(/\b(?:github_pat_|ghp_|gho_|glpat-)[A-Za-z0-9_-]{16,}\b/g, "[REDACTED]")
    .replace(/(https?:\/\/)[^\s/@]+:[^\s/@]+@/gi, "$1[REDACTED]@")
    .replace(/([?&](?:access_token|token|client_secret|password)=)[^&#\s]+/gi, "$1[REDACTED]")
    .replace(/((?:bearer|basic)\s+)[A-Za-z0-9._~+/=-]+/gi, "$1[REDACTED]")
    .replace(/((?:authorization|x-yunxiao-token|private-token|access[_-]?token|client[_-]?secret|password)\s*[:=]\s*)(?!\[REDACTED\])\S+/gi, "$1[REDACTED]");
}

export function reportFrontendWarning(message: string, cause?: unknown): void {
  const text = safeLogText(cause === undefined ? message : `${message}; error=${String(cause)}`);
  console.warn(text);
  void warn(text).catch(() => undefined);
}

export function reportFrontendError(message: string, cause?: unknown): void {
  const text = safeLogText(cause === undefined ? message : `${message}; error=${String(cause)}`);
  console.error(text);
  void error(text).catch(() => undefined);
}

/**
 * Records frontend-to-backend commands without serializing arguments, which may
 * contain access tokens, credentials, or private paths.
 */
export async function invoke<T>(
  command: string,
  args?: InvokeArgs,
  options?: InvokeOptions,
): Promise<T> {
  const startedAt = performance.now();
  if (!quietCommands.has(command)) {
    void debug(`Command started: ${command}`).catch(() => undefined);
  }
  try {
    const result = await tauriInvoke<T>(command, args, options);
    if (!quietCommands.has(command)) {
      void debug(`Command completed: ${command}; elapsed_ms=${Math.round(performance.now() - startedAt)}`).catch(() => undefined);
    }
    return result;
  } catch (cause) {
    void error(
      `Command failed: ${command}; elapsed_ms=${Math.round(performance.now() - startedAt)}; error=${safeLogText(cause)}`,
    ).catch(() => undefined);
    throw cause;
  }
}
