export const EXPORT_ROOT = "Stacker 网页对话";

/** Saves into the browser's download folder under `Stacker 网页对话/`, never asking where. */
export async function saveFile(path: string, text: string, mime: string): Promise<void> {
  const url = URL.createObjectURL(new Blob([text], { type: `${mime};charset=utf-8` }));
  try {
    await chrome.downloads.download({ url, filename: `${EXPORT_ROOT}/${path}`, conflictAction: "uniquify", saveAs: false });
  } finally {
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
  }
}
