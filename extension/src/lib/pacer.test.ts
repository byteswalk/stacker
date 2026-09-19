import { describe, expect, it } from "vitest";
import { createPacer, withPacing, sleep } from "./pacer";
import { SiteError } from "../shared/types";

describe("pacer", () => {
  it("keeps a minimum gap and backs off on rate limits", async () => {
    let clock = 0;
    const slept: number[] = [];
    const p = createPacer({ gapMs: 1000, maxBackoffMs: 8000, now: () => clock, sleep: async (ms) => { slept.push(ms); clock += ms; } });
    await p.wait();
    clock += 200;
    await p.wait();
    expect(slept).toEqual([800]);
    expect(p.rateLimited(null)).toBe(2000);
    expect(p.rateLimited(null)).toBe(4000);
    expect(p.rateLimited(null)).toBe(8000);
    expect(p.rateLimited(null)).toBe(8000);
    expect(p.rateLimited(30000)).toBe(30000);
    p.ok();
    await p.wait();
    expect(slept.at(-1)).toBe(1000);
  });

  it("withPacing retries after E_RATE then succeeds", async () => {
    const pacer = createPacer({ gapMs: 0, sleep: async () => {} });
    let attempts = 0;
    const result = await withPacing(pacer, async () => {
      attempts++;
      if (attempts === 1) {
        throw new SiteError("E_RATE", "", 5);
      }
      return "success";
    });
    expect(result).toBe("success");
    expect(attempts).toBe(2);
  });

  it("withPacing gives up after MAX_RATE_RETRIES+1 attempts", async () => {
    const pacer = createPacer({ gapMs: 0, sleep: async () => {} });
    let attempts = 0;
    const task = async () => {
      attempts++;
      throw new SiteError("E_RATE", "", 5);
    };
    await expect(withPacing(pacer, task)).rejects.toThrow(SiteError);
    expect(attempts).toBe(6); // 0-5 inclusive = 6 attempts
  });

  it("withPacing rethrows non-rate errors immediately", async () => {
    const pacer = createPacer({ gapMs: 0, sleep: async () => {} });
    let attempts = 0;
    const task = async () => {
      attempts++;
      throw new SiteError("E_BROKEN", "error");
    };
    await expect(withPacing(pacer, task)).rejects.toThrow(SiteError);
    expect(attempts).toBe(1);
  });

  it("sleep rejects with E_CANCELLED when aborted", async () => {
    const controller = new AbortController();
    const promise = sleep(1000, controller.signal);
    controller.abort();
    await expect(promise).rejects.toThrow(SiteError);
    try {
      await promise;
    } catch (e) {
      expect(e).toBeInstanceOf(SiteError);
      expect((e as SiteError).code).toBe("E_CANCELLED");
    }
  });
});
