import { callStacker, type StackerCall } from "./bridgeMessages";
import { saveFile } from "./download";

export type SaveFn = (path: string, text: string, mime: string) => Promise<void>;
type StackerFn = (call: StackerCall, payload: unknown) => Promise<unknown>;

/** Characters per saveExport message; 250k CJK characters are ~750 KB, under the 1 MB limit. */
export const STACKER_CHUNK_CHARS = 250_000;

/** Splits text into pieces without cutting a surrogate pair (that would not survive JSON on the Rust side). */
export function chunkText(text: string, size = STACKER_CHUNK_CHARS): string[] {
  if (!text) return [""];
  const out: string[] = [];
  let start = 0;
  while (start < text.length) {
    let end = Math.min(start + size, text.length);
    const last = text.charCodeAt(end - 1);
    if (end < text.length && last >= 0xd800 && last <= 0xdbff) end--;
    out.push(text.slice(start, end));
    start = end;
  }
  return out;
}

/** Saves into Stacker's export folder: the first piece picks the file, later pieces append to it. */
export function stackerSaver(call: StackerFn = callStacker, size = STACKER_CHUNK_CHARS): SaveFn {
  return async (path, text) => {
    const [first, ...rest] = chunkText(text, size);
    const saved = (await call("saveExport", { path, text: first, append: false })) as { path: string };
    for (const part of rest) await call("saveExport", { path: saved.path, text: part, append: true });
  };
}

/** Stacker's export folder when connected; the browser's downloads folder otherwise. */
export async function pickSaver(connected: () => Promise<boolean>, call: StackerFn = callStacker, download: SaveFn = saveFile): Promise<{ save: SaveFn; where: "stacker" | "downloads" }> {
  return (await connected()) ? { save: stackerSaver(call), where: "stacker" } : { save: download, where: "downloads" };
}
