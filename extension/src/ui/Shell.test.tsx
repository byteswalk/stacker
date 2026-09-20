// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { DEFAULT_PREFS, PREFS_KEY, type Prefs } from "./prefs";
import { Shell } from "./Shell";

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

async function mount() {
  await act(async () => { root.render(<Shell><p>hi</p></Shell>); });
  await act(async () => { await Promise.resolve(); });
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
