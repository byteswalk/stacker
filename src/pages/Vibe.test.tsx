import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { UnavailableSurface, type VibeSurface } from "./Vibe";

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
});
