import { describe, expect, it } from "vitest";
import { versionLabel } from "./buildLabel";

describe("the version shown in the app", () => {
  it("names the release build, or says it runs from source", () => {
    expect(versionLabel("0.3.4", "r74", false)).toBe("0.3.4 (r74)");
    expect(versionLabel("0.3.4", undefined, true)).toBe("0.3.4 (dev)");
    expect(versionLabel("0.3.4", "", false)).toBe("0.3.4");
    expect(versionLabel("", "r74", false)).toBe("");
  });
});
