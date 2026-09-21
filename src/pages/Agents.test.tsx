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

describe("where the latest version comes from", () => {
  const installed = (extra: Partial<VibeSurface>): VibeSurface => ({
    available: true, label: "ZCode 桌面端", kind: "桌面端", description: "", installed: true, status: "installed",
    version: "3.12.3", update_available: false, install_url: "https://example.invalid", docs_url: "",
    can_install: false, can_update: false, can_uninstall: true, can_open: true, health: "healthy", ...extra,
  });

  it("names the source of a latest version", () => {
    const html = renderToStaticMarkup(<SurfaceState surface={installed({ latest: "3.12.3", latest_source: "WinGet", latest_checked: true })} />);
    expect(html).toContain("最新版本：3.12.3");
    expect(html).toContain("（WinGet）");
  });

  it("says there is no public source instead of repeating the installed version", () => {
    const html = renderToStaticMarkup(<SurfaceState surface={installed({ latest_checked: true })} />);
    expect(html).toContain("最新版本：无公开渠道");
    expect(html).not.toContain("最新版本：3.12.3");
  });

  it("says a failed lookup failed, and says nothing before any lookup", () => {
    expect(renderToStaticMarkup(<SurfaceState surface={installed({ latest_checked: true, latest_error: "offline" })} />))
      .toContain("最新版本查询失败");
    const before = renderToStaticMarkup(<SurfaceState surface={installed({ latest_checked: false })} />);
    expect(before).not.toContain("最新版本");
  });
});

