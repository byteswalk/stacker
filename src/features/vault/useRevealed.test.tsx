// @vitest-environment jsdom
import { act, useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { REVEAL_MS, useRevealed } from "./useRevealed";

type Api = ReturnType<typeof useRevealed>;
let api: Api;
let host: HTMLDivElement;
let root: Root;
let errors: ReturnType<typeof vi.spyOn>;

function Probe() {
  const hook = useRevealed();
  useEffect(() => { api = hook; });
  return <span>{JSON.stringify(hook.revealed)}</span>;
}

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  errors = vi.spyOn(console, "error").mockImplementation(() => {});
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  act(() => root.render(<Probe />));
});
afterEach(() => {
  act(() => root.unmount()); host.remove();
  vi.useRealTimers(); errors.mockRestore();
});

describe("useRevealed", () => {
  it("shows a value, then hides it after 30 seconds", () => {
    act(() => api.show("Key", "v1"));
    expect(api.revealed).toEqual({ Key: "v1" });
    act(() => { vi.advanceTimersByTime(REVEAL_MS - 1); });
    expect(api.revealed).toEqual({ Key: "v1" });
    act(() => { vi.advanceTimersByTime(1); });
    expect(api.revealed).toEqual({});
  });

  it("re-arms the timer when the same key is shown again", () => {
    act(() => api.show("Key", "v1"));
    act(() => { vi.advanceTimersByTime(20_000); });
    act(() => api.show("Key", "v2"));
    act(() => { vi.advanceTimersByTime(20_000); });
    expect(api.revealed).toEqual({ Key: "v2" });
    act(() => { vi.advanceTimersByTime(10_000); });
    expect(api.revealed).toEqual({});
  });

  it("hides one key and clears them all", () => {
    act(() => { api.show("a", "1"); api.show("b", "2"); });
    act(() => api.hide("a"));
    expect(api.revealed).toEqual({ b: "2" });
    act(() => api.clear());
    expect(api.revealed).toEqual({});
    expect(vi.getTimerCount()).toBe(0);
  });

  it("drops timers on unmount and ignores a show that arrives afterwards", () => {
    act(() => api.show("Key", "v1"));
    const late = api.show;
    act(() => root.unmount());
    expect(vi.getTimerCount()).toBe(0);
    late("Key", "late");
    expect(vi.getTimerCount()).toBe(0);
    expect(errors).not.toHaveBeenCalled();
    // afterEach unmounts again: re-mount so it has something to unmount.
    root = createRoot(host);
  });
});
