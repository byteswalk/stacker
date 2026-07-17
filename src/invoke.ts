import { invoke as tauriInvoke, type InvokeArgs, type InvokeOptions } from "@tauri-apps/api/core";
import { debug, error } from "@tauri-apps/plugin-log";

const quietCommands = new Set(["settings_read_log"]);

function safeLogText(value: unknown): string {
  return String(value)
    .replace(/\bpt-[A-Za-z0-9_-]{16,}\b/g, "[REDACTED]")
    .replace(/\b(?:github_pat_|ghp_|gho_|glpat-)[A-Za-z0-9_-]{16,}\b/g, "[REDACTED]")
    .replace(/((?:authorization|x-yunxiao-token|private-token|password)\s*[:=]\s*)\S+/gi, "$1[REDACTED]");
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
  if (!quietCommands.has(command)) {
    await debug(`Command started: ${command}`).catch(() => undefined);
  }
  try {
    const result = await tauriInvoke<T>(command, args, options);
    if (!quietCommands.has(command)) {
      await debug(`Command completed: ${command}`).catch(() => undefined);
    }
    return result;
  } catch (cause) {
    await error(`Command failed: ${command}: ${safeLogText(cause)}`).catch(() => undefined);
    throw cause;
  }
}
