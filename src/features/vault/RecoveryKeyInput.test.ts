import { describe, expect, it } from "vitest";
import { emptyKey, fillKey, keyComplete, keyText } from "./RecoveryKeyInput";

const KEY = "0123-4567-89AB-CDEF-GHJK-MNPQ-RSTV-WXYZ";

describe("recovery key groups", () => {
  it("lays a pasted key into eight groups, whatever separates them", () => {
    const groups = fillKey(emptyKey(), 0, " 0123 4567-89ab cdef\nghjk-mnpq-rstv-wxyz ");
    expect(keyText(groups)).toBe(KEY);
    expect(keyComplete(groups)).toBe(true);
  });

  it("fills from the group typed in and keeps the groups before it", () => {
    const groups = fillKey(["0123", "4567", "", "", "", "", "", ""], 2, "89abcd");
    expect(groups).toEqual(["0123", "4567", "89AB", "CD", "", "", "", ""]);
    expect(keyComplete(groups)).toBe(false);
  });

  it("drops what does not fit in the eight groups", () => {
    expect(keyText(fillKey(emptyKey(), 0, KEY + "-EXTRA"))).toBe(KEY);
  });
});
