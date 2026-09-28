import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ScanLauncher } from "./ScanLauncher";

describe("ScanLauncher settings gate", () => {
  it("renders every launch entry disabled before settings resolve", () => {
    const html = renderToStaticMarkup(<ScanLauncher />);

    // 快速扫描 and 选择目录; 全盘分析 is gone because a drive root is a directory.
    expect(html.match(/<button[^>]*disabled=""/g)).toHaveLength(2);
    expect(html).toContain('aria-label="选择扫描范围"');
  });
});
