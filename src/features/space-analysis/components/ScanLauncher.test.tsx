import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ScanLauncher } from "./ScanLauncher";

describe("ScanLauncher settings gate", () => {
  it("renders every launch entry disabled before settings resolve", () => {
    const html = renderToStaticMarkup(<ScanLauncher />);

    expect(html.match(/<button[^>]*disabled=""/g)).toHaveLength(4);
    expect(html).toContain('aria-label="选择扫描范围"');
  });
});
