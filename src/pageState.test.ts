import { describe, expect, it } from "vitest";
import {
  DEFAULT_PAGE,
  normalizePage,
  readLastPage,
  saveLastPage,
} from "./pageState";

function memoryStorage(initialValue: string | null = null) {
  let value = initialValue;
  return {
    getItem: () => value,
    setItem: (_key: string, next: string) => { value = next; },
  };
}

describe("page state", () => {
  it("restores a supported page", () => {
    expect(readLastPage(memoryStorage("cleanup"))).toBe("cleanup");
  });

  it("maps legacy agent page ids to the new pages", () => {
    expect(readLastPage(memoryStorage("vibe"))).toBe("agents");
    expect(readLastPage(memoryStorage("agent-space"))).toBe("agent-data");
    expect(readLastPage(memoryStorage("agent-data"))).toBe("agent-data");
  });

  it("falls back when persisted data is missing or obsolete", () => {
    expect(normalizePage("removed-page")).toBe(DEFAULT_PAGE);
    expect(readLastPage(memoryStorage(null))).toBe(DEFAULT_PAGE);
  });

  it("persists the latest page", () => {
    const storage = memoryStorage();
    saveLastPage("settings", storage);
    expect(readLastPage(storage)).toBe("settings");
  });

  it("tolerates unavailable browser storage", () => {
    const deniedStorage = {
      getItem: () => { throw new Error("denied"); },
      setItem: () => { throw new Error("denied"); },
    };
    expect(readLastPage(deniedStorage)).toBe(DEFAULT_PAGE);
    expect(() => saveLastPage("git", deniedStorage)).not.toThrow();
  });
});
