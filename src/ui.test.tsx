// @vitest-environment jsdom
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BusyHost, BusyProvider, ConfirmModal, Modal, ToastHost, ToastProvider, useBusy, useToast } from "./ui";
import { StorageLocations } from "./StorageLocations";
import { invoke, reportFrontendWarning } from "./invoke";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("./invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  vi.spyOn(HTMLElement.prototype, "getClientRects").mockImplementation(function (this: HTMLElement) {
    return (this.closest("[hidden]") ? [] : [new DOMRect(0, 0, 100, 30)]) as unknown as DOMRectList;
  });
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => { callback(0); return 1; });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

function key(value: string, shiftKey = false) {
  act(() => { document.dispatchEvent(new KeyboardEvent("keydown", { key: value, shiftKey, bubbles: true, cancelable: true })); });
}

describe("modal keyboard behavior", () => {
  it("finishes an operation even if listener cleanup fails", async () => {
    vi.mocked(listen).mockResolvedValueOnce(() => { throw new Error("listener unavailable"); });
    let run!: ReturnType<typeof useBusy>;
    function Action() { run = useBusy(); return <BusyHost />; }
    act(() => root.render(<BusyProvider><Action /></BusyProvider>));
    await act(async () => {
      await expect(run({ title: "Working", progressEvent: "test-progress" }, async () => "done")).resolves.toBe("done");
    });
    expect(container.querySelector("[role='dialog']")).toBeNull();
    expect(reportFrontendWarning).toHaveBeenCalled();
    await act(async () => { await expect(run({ title: "Next" }, async () => "next")).resolves.toBe("next"); });
  });
  it("does not expose a close action while a confirmation is busy", () => {
    const close = vi.fn();
    act(() => root.render(<ConfirmModal title="Confirm" message="Working" busy onClose={close} onConfirm={() => {}} />));
    key("Escape");
    expect(close).not.toHaveBeenCalled();
    expect(container.querySelector(".modalhd button")).toBeNull();
    expect(document.activeElement).toBe(container.querySelector("[role='dialog']"));
    key("Tab");
    expect(document.activeElement).toBe(container.querySelector("[role='dialog']"));
  });

  it("closes only the top dialog and restores focus to its opener", () => {
    function Nested() {
      const [child, setChild] = useState(false);
      return <Modal title="Parent" onClose={closeParent}>
        <button id="open-child" onClick={() => setChild(true)}>Open</button>
        {child && <Modal title="Child" onClose={() => setChild(false)}><button>Child action</button></Modal>}
      </Modal>;
    }
    const closeParent = vi.fn();
    act(() => root.render(<Nested />));
    const opener = container.querySelector<HTMLButtonElement>("#open-child")!;
    opener.focus();
    act(() => opener.click());
    expect(container.querySelectorAll("[role='dialog']")).toHaveLength(2);
    key("Escape");
    expect(container.querySelectorAll("[role='dialog']")).toHaveLength(1);
    expect(closeParent).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(opener);
  });

  it("traps focus without including hidden controls", () => {
    act(() => root.render(<Modal title="Focus"><button id="first">First</button><button hidden>Hidden</button><button id="last">Last</button></Modal>));
    const first = container.querySelector("#first");
    expect(document.activeElement).toBe(first);
    key("Tab", true);
    expect(document.activeElement).toBe(container.querySelector("#last"));
    key("Tab");
    expect(document.activeElement).toBe(first);
  });

  it("prevents Escape from reaching the confirmation under a busy dialog", async () => {
    let finish!: () => void;
    const task = new Promise<void>((resolve) => { finish = resolve; });
    const close = vi.fn();
    function Action() {
      const busy = useBusy();
      return <ConfirmModal title="Confirm" message="Ready" onClose={close} onConfirm={() => { void busy({ title: "Working" }, () => task); }} />;
    }
    act(() => root.render(<BusyProvider><Action /><BusyHost /></BusyProvider>));
    await act(async () => container.querySelector<HTMLButtonElement>(".modalft .pr")!.click());
    key("Escape");
    expect(close).not.toHaveBeenCalled();
    const busyDialog = container.querySelector("[aria-busy='true']");
    expect(busyDialog).not.toBeNull();
    expect(document.activeElement).toBe(busyDialog);
    key("Tab");
    expect(document.activeElement).toBe(busyDialog);
    await act(async () => finish());
    expect(container.querySelector("[aria-busy='true']")).toBeNull();
  });
});

describe("toast lifecycle", () => {
  it("caps the stack, merges repeated messages, refreshes duration, and clears timers", () => {
    let push!: ReturnType<typeof useToast>;
    function Producer() { push = useToast(); return <ToastHost />; }
    act(() => root.render(<ToastProvider><Producer /></ToastProvider>));
    act(() => { for (const label of ["one", "two", "three", "four"]) push(label); });
    expect(container.querySelectorAll(".toast")).toHaveLength(3);
    expect(container.textContent).not.toContain("one");
    act(() => vi.advanceTimersByTime(3000));
    act(() => push("four"));
    expect(container.querySelectorAll(".toast")).toHaveLength(3);
    act(() => vi.advanceTimersByTime(600));
    expect(container.querySelectorAll(".toast")).toHaveLength(1);
    expect(container.textContent).toContain("four");
    act(() => root.render(null));
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("storage mutation lifecycle", () => {
  const row = { id: "cargo-home", ecosystem: "rust", label: "Cargo", description: "Cache", path: "D:/old", default_path: "D:/default", source: "env", size_bytes: 0, advanced: true };

  it("updates the row from the command result before dismissing progress, without a fallible second query", async () => {
    let finish!: (value: typeof row) => void;
    const pending = new Promise<typeof row>((resolve) => { finish = resolve; });
    vi.mocked(invoke).mockReset().mockImplementation(async (command) => {
      if (command === "storage_locations") return [row];
      if (command === "storage_apply") return pending;
      throw new Error(`Unexpected command: ${command}`);
    });
    vi.mocked(open).mockResolvedValue("D:/new");
    await act(async () => root.render(<ToastProvider><BusyProvider><StorageLocations ecosystem="rust" /><BusyHost /><ToastHost /></BusyProvider></ToastProvider>));
    await act(async () => container.querySelector<HTMLButtonElement>(".storage-actions .pr")!.click());
    await act(async () => container.querySelector<HTMLButtonElement>(".modalft .pr")!.click());
    expect(container.querySelector("[aria-busy='true']")).not.toBeNull();
    expect(container.querySelector(".storage-path")?.textContent).toBe("D:/old");
    await act(async () => finish({ ...row, path: "D:/new" }));
    expect(container.querySelector(".storage-path")?.textContent).toBe("D:/new");
    expect(container.querySelector("[role='dialog']")).toBeNull();
    expect(container.querySelector(".toast.ok")).not.toBeNull();
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === "storage_locations")).toHaveLength(1);
  });

  it("reports a failed folder picker instead of leaking an unhandled rejection", async () => {
    vi.mocked(invoke).mockReset().mockResolvedValue([row]);
    vi.mocked(open).mockRejectedValue(new Error("dialog unavailable"));
    await act(async () => root.render(<ToastProvider><StorageLocations ecosystem="rust" /><ToastHost /></ToastProvider>));
    await act(async () => container.querySelector<HTMLButtonElement>(".storage-actions .pr")!.click());
    expect(container.querySelector(".toast.err")?.textContent).toContain("dialog unavailable");
    expect(container.querySelector("[role='dialog']")).toBeNull();
  });
});
