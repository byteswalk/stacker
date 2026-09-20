// @vitest-environment jsdom
import { act, useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_PREFS, loadPrefs, PREFS_KEY, type Mode, type Prefs } from "./prefs";
import { Shell, usePrefs } from "./Shell";

vi.mock("../lib/bridgeMessages", () => ({
  bridgeStatus: vi.fn(),
  callStacker: vi.fn(),
}));
const { bridgeStatus, callStacker } = await import("../lib/bridgeMessages");

const stackerOn = (theme: string | null) =>
  vi.mocked(bridgeStatus).mockResolvedValue({ connected: true, pending: 0, lastSyncAt: null, error: "", theme });
const stackerOff = () =>
  vi.mocked(bridgeStatus).mockResolvedValue({ connected: false, pending: 0, lastSyncAt: null, error: "", theme: null });

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

/** A prefers-color-scheme stub whose answer can change, like the system setting does. */
let light = true;
let listeners: (() => void)[] = [];
function stubMatchMedia() {
  window.matchMedia = ((query: string) => ({
    // A real MediaQueryList keeps `matches` up to date on the same object.
    get matches() { return query.includes("light") ? light : !light; },
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: (_: string, fn: EventListenerOrEventListenerObject) => { listeners.push(fn as () => void); },
    removeEventListener: (_: string, fn: EventListenerOrEventListenerObject) => { listeners = listeners.filter((l) => l !== fn); },
    dispatchEvent: () => false,
  })) as typeof window.matchMedia;
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.clearAllMocks();
  stackerOff();
  light = true;
  listeners = [];
  stubMatchMedia();
  localStorage.clear();
  delete document.documentElement.dataset.theme;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

// Reaches the two things the pages themselves use: picking an appearance, and asking again.
let pick: (mode: Mode) => void = () => {};
let askAgain: () => void = () => {};
function Probe() {
  const api = usePrefs();
  useEffect(() => { pick = (mode) => api.update({ mode }); askAgain = api.reconnect; }, [api]);
  return <p>hi</p>;
}

async function mount() {
  await act(async () => { root.render(<Shell><Probe /></Shell>); });
  await settle();
}

async function settle() {
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });
}

describe("Shell appearance", () => {
  it("follows the system by default and keeps following it when the system changes", async () => {
    expect(DEFAULT_PREFS.mode).toBe("auto");
    await mount();
    expect(document.documentElement.dataset.theme).toBe("light");

    light = false;
    await act(async () => { listeners.forEach((fn) => fn()); });
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("pins the theme the user picked, whatever the system says", async () => {
    const pinned: Prefs = { lang: "auto", mode: "dark" };
    localStorage.setItem(PREFS_KEY, JSON.stringify(pinned));
    await mount();
    expect(document.documentElement.dataset.theme).toBe("dark");

    light = false;
    await act(async () => { listeners.forEach((fn) => fn()); });
    expect(document.documentElement.dataset.theme).toBe("dark");
  });
});

describe("Shell and Stacker keep one appearance", () => {
  it("adopts Stacker's appearance while bridged", async () => {
    localStorage.setItem(PREFS_KEY, JSON.stringify({ lang: "auto", mode: "auto" } satisfies Prefs));
    stackerOn("dark");
    await mount();
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(await loadPrefs()).toMatchObject({ mode: "dark" });
    // Following Stacker is not a change of its own: nothing is sent back.
    expect(callStacker).not.toHaveBeenCalled();
  });

  it("hands an appearance picked here to Stacker, and an older answer cannot undo it", async () => {
    localStorage.setItem(PREFS_KEY, JSON.stringify({ lang: "auto", mode: "dark" } satisfies Prefs));
    stackerOn("dark");
    vi.mocked(callStacker).mockResolvedValue({ theme: "light" });
    await mount();

    await act(async () => { pick("light"); });
    await settle();
    expect(callStacker).toHaveBeenCalledWith("setTheme", { theme: "light" });
    expect(document.documentElement.dataset.theme).toBe("light");

    // A status from before the change still says dark; the pick must survive it.
    await act(async () => { askAgain(); });
    await settle();
    expect(document.documentElement.dataset.theme).toBe("light");

    // Stacker confirms, and following it resumes.
    stackerOn("light");
    await act(async () => { askAgain(); });
    await settle();
    stackerOn("system");
    await act(async () => { askAgain(); });
    await settle();
    expect(await loadPrefs()).toMatchObject({ mode: "auto" });
  });

  it("keeps the appearance local when Stacker is not there", async () => {
    vi.mocked(callStacker).mockRejectedValue(new Error("E_NOT_CONNECTED"));
    await mount();
    await act(async () => { pick("dark"); });
    await settle();
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(await loadPrefs()).toMatchObject({ mode: "dark" });
  });
});
