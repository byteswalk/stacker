import { describe, expect, it } from "vitest";
import { VIBE_FRESH_MS, vibeIsFresh } from "./catalogStore";

describe("vibeIsFresh", () => {
  const now = Date.parse("2026-11-07T06:00:00Z");

  it("reuses a detection this run finished within ten minutes", () => {
    expect(vibeIsFresh(now - 60_000, now)).toBe(true);
  });

  it("detects again once that is older, or when this run has not detected yet", () => {
    expect(vibeIsFresh(now - VIBE_FRESH_MS - 1, now)).toBe(false);
    expect(vibeIsFresh(null, now)).toBe(false);
  });
});
