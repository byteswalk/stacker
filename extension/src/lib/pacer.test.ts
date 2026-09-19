import { describe, expect, it } from "vitest";
import { createPacer } from "./pacer";

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
});
