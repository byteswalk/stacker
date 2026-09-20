// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getTheme, setTheme, THEME_CHANGED_EVENT, watchSharedTheme } from "./theme";

let stop: () => void = () => {};

beforeEach(() => {
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});
afterEach(() => stop());

/** watchSharedTheme also checks on focus, which is the quickest way to make it look. */
async function check() {
  window.dispatchEvent(new Event("focus"));
  await new Promise((resolve) => setTimeout(resolve, 0));
}

describe("appearance shared with the browser extension", () => {
  it("applies an appearance changed outside this window and says so", async () => {
    setTheme("dark");
    const seen = vi.fn();
    window.addEventListener(THEME_CHANGED_EVENT, seen);
    stop = watchSharedTheme(async () => "light");

    await check();
    expect(getTheme()).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(seen).toHaveBeenCalledTimes(1);

    // Already in step: no second announcement.
    await check();
    expect(seen).toHaveBeenCalledTimes(1);
    window.removeEventListener(THEME_CHANGED_EVENT, seen);
  });

  it("ignores an unreadable or nonsense answer and keeps the current appearance", async () => {
    setTheme("dark");
    stop = watchSharedTheme(async () => "chartreuse");
    await check();
    expect(getTheme()).toBe("dark");

    stop();
    stop = watchSharedTheme(() => Promise.reject(new Error("no backend")));
    await check();
    expect(getTheme()).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("does not ask while the window is hidden", async () => {
    const read = vi.fn(async () => "light");
    const hidden = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    stop = watchSharedTheme(read);
    await check();
    expect(read).not.toHaveBeenCalled();
    hidden.mockRestore();
  });
});
