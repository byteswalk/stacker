import { SiteError } from "../shared/types";

export const MAX_RATE_RETRIES = 5;

export interface Pacer { wait(): Promise<void>; rateLimited(retryAfterMs: number | null): number; ok(): void }

export function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) return reject(new SiteError("E_CANCELLED"));
    const timer = setTimeout(resolve, ms);
    signal?.addEventListener("abort", () => { clearTimeout(timer); reject(new SiteError("E_CANCELLED")); }, { once: true });
  });
}

/** One request per `gapMs`; after a rate limit the gap doubles up to `maxBackoffMs` (or follows Retry-After). */
export function createPacer({ gapMs = 1500, maxBackoffMs = 60_000, sleep: nap = sleep, now = Date.now }: {
  gapMs?: number; maxBackoffMs?: number; sleep?: (ms: number) => Promise<void>; now?: () => number;
} = {}): Pacer {
  let last = -Infinity;
  let gap = gapMs;
  return {
    async wait() {
      const due = last + gap - now();
      if (due > 0) await nap(due);
      last = now();
    },
    rateLimited(retryAfterMs) {
      gap = retryAfterMs ?? Math.min(Math.max(gap, gapMs) * 2, maxBackoffMs);
      return gap;
    },
    ok() { gap = gapMs; },
  };
}

/** Waits its turn, runs the task, and on E_RATE backs off and retries (up to MAX_RATE_RETRIES); other errors are rethrown. */
export async function withPacing<T>(pacer: Pacer, task: () => Promise<T>, signal?: AbortSignal): Promise<T> {
  for (let attempt = 0; ; attempt++) {
    await pacer.wait();
    try {
      const v = await task();
      pacer.ok();
      return v;
    } catch (err) {
      if (!(err instanceof SiteError) || err.code !== "E_RATE" || attempt >= MAX_RATE_RETRIES) {
        throw err;
      }
      await sleep(pacer.rateLimited(err.retryAfterMs), signal);
    }
  }
}
