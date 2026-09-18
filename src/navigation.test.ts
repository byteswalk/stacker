import { describe, expect, it } from "vitest";
import { ALL_NAV_ITEMS, NAV_SECTIONS, readCollapsedSections, sectionKeyOf, toggleSection } from "./navigation";
import { PAGE_IDS } from "./pageState";
import { t } from "./i18n";

const PRODUCT_PAGES = new Set(["git", "python", "php", "node", "java", "maven", "gradle", "go", "rust"]);

describe("sidebar navigation", () => {
  it("lists every page exactly once", () => {
    expect(ALL_NAV_ITEMS.map((item) => item.id).sort()).toEqual([...PAGE_IDS].sort());
  });

  it("puts agent management first with five-character sections and four-character items", () => {
    expect(NAV_SECTIONS[1].labelKey).toBe("nav.section.agents");
    for (const section of NAV_SECTIONS) {
      if (section.labelKey) expect([...t(section.labelKey, "zh-CN")], section.labelKey).toHaveLength(5);
      for (const item of section.items) {
        if (!PRODUCT_PAGES.has(item.id)) expect([...t(item.labelKey, "zh-CN")], item.id).toHaveLength(4);
      }
    }
    for (const item of ALL_NAV_ITEMS.filter((entry) => !PRODUCT_PAGES.has(entry.id))) {
      expect([...t(item.labelKey, "zh-CN")], item.id).toHaveLength(4);
    }
  });

  it("remembers collapsed sections and finds the section of a page", () => {
    let stored: string | null = null;
    const storage = { getItem: () => stored, setItem: (_: string, value: string) => { stored = value; } };
    expect(readCollapsedSections(storage)).toEqual([]);
    const next = toggleSection([], "nav.section.devEnv", storage);
    expect(next).toEqual(["nav.section.devEnv"]);
    expect(readCollapsedSections(storage)).toEqual(["nav.section.devEnv"]);
    expect(toggleSection(next, "nav.section.devEnv", storage)).toEqual([]);
    expect(sectionKeyOf("rust")).toBe("nav.section.devEnv");
    expect(sectionKeyOf("overview")).toBeNull();
    stored = "not json";
    expect(readCollapsedSections(storage)).toEqual([]);
  });
});
