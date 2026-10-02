import { describe, expect, it } from "vitest";
import { ALL_NAV_ITEMS, NAV_FOOT, NAV_SECTIONS, initialCollapsedSections, sectionKeyOf, toggleSection } from "./navigation";
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

  it("opens with only 智能体管理 expanded, and finds the section of a page", () => {
    const collapsed = initialCollapsedSections();
    expect(collapsed).not.toContain("nav.section.agents");
    expect(collapsed).toEqual(expect.arrayContaining(["nav.section.devEnv", "nav.section.system"]));
    const next = toggleSection(collapsed, "nav.section.devEnv");
    expect(next).not.toContain("nav.section.devEnv");
    expect(toggleSection(next, "nav.section.devEnv")).toContain("nav.section.devEnv");
    expect(sectionKeyOf("rust")).toBe("nav.section.devEnv");
    expect(sectionKeyOf("overview")).toBeNull();
  });

  it("puts 密钥保管 third under 网络与存储, and keeps 配置备份 and 偏好设置 at the foot", () => {
    expect(NAV_FOOT.map((item) => item.id)).toEqual(["history", "settings"]);
    expect(NAV_SECTIONS.find((section) => section.labelKey === "nav.section.system")!.items.map((item) => item.id)).toEqual(["proxy", "cleanup", "vault"]);
    expect(sectionKeyOf("vault")).toBe("nav.section.system");
    expect(t("nav.vault", "zh-CN")).toBe("密钥保管");
    expect(t("nav.vault", "en-US")).toBe("Key Vault");
  });
});
