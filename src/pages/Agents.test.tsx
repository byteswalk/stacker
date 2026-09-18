import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { SurfaceState, UnavailableSurface, type VibeSurface } from "./Agents";

describe("unavailable agent interfaces", () => {
  it.each(["cli", "desktop"] as const)("renders a complete %s slot without installation actions", (target) => {
    const surface: VibeSurface = {
      available: false, label: "Example Agent", kind: target,
      description: "This interface is not offered for Windows.", installed: false,
      status: "missing", update_available: false, install_url: "", docs_url: "",
      can_install: false, can_update: false, can_uninstall: false, can_open: false,
    };
    const html = renderToStaticMarkup(<UnavailableSurface target={target} surface={surface} />);
    expect(html).toContain("vtool-surface unavailable");
    expect(html).toContain("Example Agent");
    expect(html).toContain(surface.description);
    expect(html).toContain("暂不可用");
    expect(html).not.toContain("<button");
    expect(html).not.toContain("未安装");
  });

  it("shows a broken install with its reason and other installs", () => {
    const surface: VibeSurface = {
      available: true, label: "Claude Code CLI", kind: "CLI", description: "", installed: false,
      status: "broken", health: "broken", broken_reason: "不是有效的 Windows 程序", update_available: false,
      install_url: "", docs_url: "", can_install: true, can_update: false, can_uninstall: true, can_open: false,
      can_repair: true, other_installs: [{ path: "C:/winget/claude.exe", healthy: true, version: "2.1.227" }],
    };
    const html = renderToStaticMarkup(<SurfaceState surface={surface} />);
    expect(html).toContain("已损坏");
    expect(html).toContain("不是有效的 Windows 程序");
    expect(html).toContain("另有 1 个安装");
  });
});
