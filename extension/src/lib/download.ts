export const EXPORT_ROOT = "Stacker 网页对话";
export const DOWNLOAD_TIMEOUT_MS = 60_000;

export type DownloadsApi = Pick<typeof chrome.downloads, "download" | "onChanged">;

/**
 * Saves into the browser's download folder under `Stacker 网页对话/`, never asking where.
 * Resolves only once the browser reports the file complete; rejects if it is interrupted or times out.
 */
export async function saveWith(downloads: DownloadsApi, path: string, text: string, mime: string, timeoutMs = DOWNLOAD_TIMEOUT_MS): Promise<void> {
  const url = URL.createObjectURL(new Blob([text], { type: `${mime};charset=utf-8` }));
  // Events can arrive before download() hands back the id, so remember every final state.
  const finals = new Map<number, { state: string; error: string }>();
  let wake = () => {};
  const listener = (d: chrome.downloads.DownloadDelta) => {
    const state = d.state?.current;
    if (state !== "complete" && state !== "interrupted") return;
    finals.set(d.id, { state, error: d.error?.current ?? "unknown" });
    wake();
  };
  downloads.onChanged.addListener(listener);
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const id = await downloads.download({ url, filename: `${EXPORT_ROOT}/${path}`, conflictAction: "uniquify", saveAs: false });
    if (typeof id !== "number") throw new Error("download did not start");
    await new Promise<void>((resolve, reject) => {
      wake = () => {
        const f = finals.get(id);
        if (!f) return;
        if (f.state === "complete") resolve();
        else reject(new Error(`download interrupted: ${f.error}`));
      };
      timer = setTimeout(() => reject(new Error("download timed out")), timeoutMs);
      wake();
    });
  } finally {
    clearTimeout(timer);
    downloads.onChanged.removeListener(listener);
    URL.revokeObjectURL(url);
  }
}

export const saveFile = (path: string, text: string, mime: string): Promise<void> => saveWith(chrome.downloads, path, text, mime);
