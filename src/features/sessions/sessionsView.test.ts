import { describe, expect, it } from "vitest";
import { currentSelection, formatAge, toggleSelection } from "./sessionsView";

describe("session view helpers", () => {
  it("toggles and trims selections", () => {
    expect(toggleSelection(["a"], "b")).toEqual(["a", "b"]);
    expect(toggleSelection(["a", "b"], "a")).toEqual(["b"]);
    expect(currentSelection(["a", "x"], ["a", "b"])).toEqual(["a"]);
  });

  it("formats ages", () => {
    expect(formatAge(1000, 1030)).toBe("刚刚");
    expect(formatAge(1000, 1000 + 3 * 3600)).toBe("3 小时前");
    expect(formatAge(1000, 1000 + 2 * 86400)).toBe("2 天前");
  });
});
