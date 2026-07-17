import { describe, expect, it } from "vitest";
import { ecosystemUpdateFromInfo } from "./updateHelpers";

describe("ecosystem update notifications", () => {
  it("creates a Git update notice with the selected source", () => {
    expect(ecosystemUpdateFromInfo("git", "Git", {
      current: "2.55.0.windows.2",
      latest: "2.56.0.windows.1",
      has_update: true,
      source_name: "npmmirror",
    })).toEqual({
      id: "git",
      name: "Git",
      current: "2.55.0.windows.2",
      latest: "2.56.0.windows.1",
      source: "npmmirror",
    });
  });

  it("ignores an up-to-date result", () => {
    expect(ecosystemUpdateFromInfo("git", "Git", {
      current: "2.56.0",
      latest: "2.56.0",
      has_update: false,
      source_name: "官方",
    })).toBeNull();
  });
});
