import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { SpaceScanSnapshot } from "../store";
import type { ScanTaskState } from "../types";
import { ScanHeader } from "./ScanHeader";

function snapshot(state: "idle" | "starting" | ScanTaskState): SpaceScanSnapshot {
  const starting = state === "starting";
  const idle = state === "idle";
  return {
    taskId: idle || starting ? null : "scan-1",
    request: idle || starting ? null : { mode: "quick", targets: [] },
    pendingRequest: starting ? { mode: "directories", targets: ["C:\\work"] } : null,
    progress: idle || starting ? null : {
      taskId: "scan-1",
      state,
      scannedFiles: 1,
      scannedDirectories: 1,
      accountedBytes: 4096,
      skippedPaths: 0,
      elapsedMs: 1000,
      currentPath: "C:\\work",
    },
    result: null,
    error: null,
  };
}

describe("ScanHeader stable shell", () => {
  it.each(["idle", "starting", "running", "completed"] as const)(
    "renders identical grid and metric dimensions for %s",
    (state) => {
      const html = renderToStaticMarkup(
        <ScanHeader scan={snapshot(state)} onCancel={() => undefined} />,
      );

      expect(html).toContain('class="clhero space-scan-header"');
      expect(html).toContain(`data-phase="${state}"`);
      expect(html.match(/class="scan-metric"/g)).toHaveLength(5);
      expect(html).toContain('class="scan-header-progress"');
      expect(html).toContain('class="scan-header-actions"');
    },
  );
});
