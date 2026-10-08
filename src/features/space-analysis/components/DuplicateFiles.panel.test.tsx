// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../../invoke";
import { DuplicateFiles, resetDuplicateSearch } from "./DuplicateFiles";

vi.mock("../../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));

const report = {
  groups: [{ bytes: 20 * 1024 * 1024, wasted: 20 * 1024 * 1024, verified: true, paths: [String.raw`D:\a\movie.mp4`, String.raw`D:\b\movie.mp4`] }],
  wasted: 20 * 1024 * 1024, complete: true,
};

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  resetDuplicateSearch();
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation((async (command: string, args?: { paths?: string[] }) => {
    if (command === "space_duplicates") return report;
    if (command === "space_file_times") return args!.paths!.map(() => ({ created: new Date(2026, 0, 2, 3, 4).getTime(), modified: new Date(2026, 5, 6, 7, 8).getTime() }));
    if (command === "space_protected_paths") return args!.paths!.map(() => false);
    return null;
  }) as typeof invoke);
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

const buttons = () => [...host.querySelectorAll<HTMLButtonElement>("button")];
const button = (text: string) => buttons().find((item) => item.textContent?.includes(text))!;
const searches = () => vi.mocked(invoke).mock.calls.filter(([command]) => command === "space_duplicates");

describe("duplicate files", () => {
  it("searches only when asked, shows when each copy was made and changed, and keeps the result across tabs", async () => {
    await act(async () => { root.render(<DuplicateFiles taskId="t1" />); });
    await act(async () => { button("100 MB 以上").click(); });
    expect(searches()).toHaveLength(0);
    await act(async () => { button("查找重复文件").click(); });
    expect(searches()).toEqual([["space_duplicates", { taskId: "t1", minBytes: 100 * 1024 * 1024 }]]);
    expect(host.textContent).toContain("2026-01-02 03:04");
    expect(host.textContent).toContain("2026-06-06 07:08");

    // Another tab, then back: the same result, no new search.
    await act(async () => { root.render(<div />); });
    await act(async () => { root.render(<DuplicateFiles taskId="t1" />); });
    expect(host.textContent).toContain("movie.mp4");
    expect(searches()).toHaveLength(1);

    // A new scan starts afresh.
    await act(async () => { root.render(<DuplicateFiles taskId="t2" />); });
    expect(host.textContent).not.toContain("movie.mp4");
  });
});
