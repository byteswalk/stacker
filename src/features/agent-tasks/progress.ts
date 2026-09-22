/** How far a task has got, when its latest log line says so. */
export type Progress = { ratio: number; percent: number };

const UNIT: Record<string, number> = {
  b: 1, kb: 1024, mb: 1024 ** 2, gb: 1024 ** 3, kib: 1024, mib: 1024 ** 2, gib: 1024 ** 3,
};

function measured(ratio: number): Progress | null {
  if (!Number.isFinite(ratio) || ratio < 0) return null;
  const clamped = Math.min(1, ratio);
  return { ratio: clamped, percent: Math.round(clamped * 100) };
}

/**
 * Real progress in a task's latest log line. Stacker's own downloader writes
 * "正在下载 45% · 12.3/27.0 MB"; a tool printing "12.0 MB / 48.0 MB" is read too.
 * npm prints nothing measurable, and WinGet draws no progress at all once its output is
 * captured instead of shown in a console, so those tasks keep the sliding bar rather than
 * a made-up number.
 */
export function parseProgress(line: string | null | undefined): Progress | null {
  if (!line) return null;
  // Stacker's downloader and WinGet's bars: "正在下载 45%"; a Store install: "正在处理 45%".
  const ours = /正在(?:下载|处理)\s+(\d{1,3}(?:\.\d+)?)%/.exec(line);
  if (ours) return measured(Number(ours[1]) / 100);

  const sizes = /(\d+(?:\.\d+)?)\s*(KiB|MiB|GiB|KB|MB|GB|B)\s*\/\s*(\d+(?:\.\d+)?)\s*(KiB|MiB|GiB|KB|MB|GB|B)\b/i.exec(line);
  if (sizes) {
    const done = Number(sizes[1]) * UNIT[sizes[2].toLowerCase()];
    const total = Number(sizes[3]) * UNIT[sizes[4].toLowerCase()];
    if (total > 0) return measured(done / total);
  }
  return null;
}
