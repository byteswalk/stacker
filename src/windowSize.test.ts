import { describe, expect, it } from "vitest";
import {
  MINIMUM_WINDOW_SIZE,
  normalizeClientSize,
  normalizeWindowSize,
} from "./windowSize";

describe("normalizeWindowSize", () => {
  it("rejects missing and invalid values", () => {
    expect(normalizeWindowSize(null)).toBeNull();
    expect(normalizeWindowSize({ width: "1280", height: 720 })).toBeNull();
    expect(normalizeWindowSize({ width: Number.NaN, height: 720 })).toBeNull();
  });

  it("rounds valid dimensions", () => {
    expect(normalizeWindowSize({ width: 1280.4, height: 719.6 })).toEqual({
      width: 1280,
      height: 720,
    });
  });

  it("enforces the minimum window size", () => {
    expect(normalizeWindowSize({ width: 600, height: 400 })).toEqual(
      MINIMUM_WINDOW_SIZE,
    );
  });

  it("converts physical client size to logical pixels", () => {
    expect(normalizeClientSize({ width: 1600, height: 900 }, 1.25)).toEqual({
      width: 1280,
      height: 720,
    });
  });
});
