import { describe, expect, it } from "vitest";
import { ecoLine, ecoTone, ecoVersion } from "./EcoPanel";

describe("ecosystem panel", () => {
  it("finds the version in a tool's own words", () => {
    expect(ecoVersion("git version 2.56.0.windows.3")).toBe("2.56.0");
    expect(ecoVersion("pyenv 3.1.1 · Python 3.13.16")).toBe("3.13.16");
    expect(ecoVersion("go version go1.25.0 windows/amd64")).toBe("1.25.0");
    expect(ecoVersion("未检测到 Git")).toBe("未检测到 Git");
  });

  it("names the source only when it is known", () => {
    expect(ecoLine({ status: "ok", summary: "git version 2.56.0" }, "官方")).toBe("2.56.0 · 官方");
    expect(ecoLine({ status: "ok", summary: "git version 2.56.0" }, "未识别")).toBe("2.56.0");
    expect(ecoLine({ status: "missing", summary: "未检测到可用版本" }, "—")).toBe("未检测到可用版本");
  });

  it("lets a checkup item override what the ecosystem says of itself", () => {
    expect(ecoTone({ status: "ok" }, true)).toBe("warn");
    expect(ecoTone({ status: "missing" }, false)).toBe("idle");
    expect(ecoTone({ status: "ok" }, false)).toBe("ok");
    // Did not answer in time: not the same as not being there.
    expect(ecoTone({ status: "unknown" }, false)).toBe("unknown");
  });
});
