import { describe, expect, it } from "vitest";
import { VIBE_FRESH_MS, vibeIsFresh, type VibeCache } from "./catalogStore";

const cache = (checked: boolean, checkedAt: string | null): VibeCache => ({ tools: [], loading: false, checked, checkedAt });

describe("vibeIsFresh", () => {
  const now = Date.parse("2026-11-07T06:00:00Z");

  it("reuses a full detection made within ten minutes", () => {
    expect(vibeIsFresh(cache(true, new Date(now - 60_000).toISOString()), now)).toBe(true);
  });

  it("scans again once the detection is older, or was never a full one", () => {
    expect(vibeIsFresh(cache(true, new Date(now - VIBE_FRESH_MS - 1).toISOString()), now)).toBe(false);
    expect(vibeIsFresh(cache(false, new Date(now).toISOString()), now)).toBe(false);
    expect(vibeIsFresh(cache(true, null), now)).toBe(false);
  });
});
