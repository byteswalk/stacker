import { describe, expect, it } from "vitest";
import type { VibeSurface } from "./catalogStore";
import { shortVersion, tileLine } from "./tileState";

const surface = (patch: Partial<VibeSurface>): VibeSurface => ({
  available: true,
  label: "Codex CLI",
  kind: "CLI",
  description: "",
  installed: true,
  status: "installed",
  version: "codex-cli 0.155.1",
  update_available: false,
  install_url: "",
  docs_url: "",
  can_install: true,
  can_update: true,
  can_uninstall: true,
  can_open: true,
  health: "healthy",
  ...patch,
} as VibeSurface);

describe("thumbnail lines", () => {
  it("shortens versions to the number people compare", () => {
    expect(shortVersion("codex-cli 0.155.1")).toBe("0.155.1");
    expect(shortVersion("2.1.268 (Claude Code)")).toBe("2.1.268");
    expect(shortVersion("26.915.4065.0")).toBe("26.915.4065.0");
    expect(shortVersion(null)).toBeNull();
  });

  it("says in a few characters how each surface stands", () => {
    expect(tileLine(surface({}), false)).toEqual({ tone: "ok", text: "0.155.1" });
    expect(tileLine(surface({ update_available: true, status: "update", latest: "0.156.0" }), false))
      .toEqual({ tone: "update", text: "0.155.1 → 0.156.0" });
    expect(tileLine(surface({ installed: false, status: "missing", version: null }), false))
      .toEqual({ tone: "missing", text: "未安装" });
    expect(tileLine(surface({ health: "broken", status: "broken" }), false).tone).toBe("broken");
    expect(tileLine(surface({ available: false }), false)).toEqual({ tone: "na", text: "不支持" });
    expect(tileLine(surface({}), true)).toEqual({ tone: "busy", text: "处理中" });
  });
});
