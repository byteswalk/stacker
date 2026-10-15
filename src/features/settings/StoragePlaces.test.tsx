// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { StoragePlaces } from "./StoragePlaces";
import { ToastProvider } from "../../ui";
import { invoke } from "../../invoke";

vi.mock("../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
});

describe("StoragePlaces", () => {
  it("lists each folder with its path and size, and opens one by its key only", async () => {
    vi.mocked(invoke).mockReset().mockImplementation(async (command) => {
      if (command === "storage_places") return [
        { key: "runtimes", path: "C:\\Users\\me\\AppData\\Local\\Stacker\\runtimes", exists: true, bytes: 3 * 1024 ** 3 },
        { key: "tools", path: "C:\\Users\\me\\AppData\\Local\\Stacker\\tools", exists: false, bytes: 0 },
      ];
      return undefined;
    });
    await act(async () => root.render(<ToastProvider><StoragePlaces /></ToastProvider>));
    const rows = container.querySelectorAll(".storage-place");
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain("运行时");
    expect(rows[0].textContent).toContain("C:\\Users\\me\\AppData\\Local\\Stacker\\runtimes");
    expect(rows[0].textContent).toContain("3.0 GB");
    expect(rows[1].querySelector("button")!.disabled).toBe(true);
    await act(async () => rows[0].querySelector("button")!.click());
    expect(invoke).toHaveBeenCalledWith("storage_place_open", { key: "runtimes" });
  });
});
