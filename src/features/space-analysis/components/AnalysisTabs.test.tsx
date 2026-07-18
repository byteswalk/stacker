import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { ScanRequest, VolumeInfo } from "../types";
import { ANALYSIS_TABS, matchedFreeBytes } from "./AnalysisTabs";

describe("AnalysisTabs", () => {
  it("exposes every implemented analysis view", () => {
    const html = renderToStaticMarkup(
      <div>{ANALYSIS_TABS.map((tab) => <span key={tab}>{tab}</span>)}</div>,
    );
    expect(html).toContain("overview");
    expect(html).toContain("directories");
    expect(html).toContain("large-files");
    expect(html).toContain("development-artifacts");
    expect(html).toContain("cache-downloads");
    expect(html).toContain("changes");
  });
});

describe("matchedFreeBytes", () => {
  const volumes: VolumeInfo[] = [
    { root: "C:\\", label: "System", fileSystem: "NTFS", totalBytes: 1000, freeBytes: 300, fixed: true },
    { root: "D:\\", label: "Data", fileSystem: "NTFS", totalBytes: 2000, freeBytes: 700, fixed: true },
  ];

  it("sums free space only when every selected fixed drive still matches", () => {
    const request: ScanRequest = { mode: "drives", targets: ["c:\\", "D:\\"] };
    expect(matchedFreeBytes(request, volumes)).toBe(1000);
  });

  it("returns unavailable for directory scans or stale drive selections", () => {
    expect(matchedFreeBytes({ mode: "directories", targets: ["C:\\work"] }, volumes)).toBeNull();
    expect(matchedFreeBytes({ mode: "drives", targets: ["E:\\"] }, volumes)).toBeNull();
    expect(matchedFreeBytes({ mode: "drives", targets: [] }, volumes)).toBeNull();
  });
});
