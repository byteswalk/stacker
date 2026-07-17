import { describe, expect, it } from "vitest";
import { normalizeLocale, translateText } from "./i18n";

describe("internationalization", () => {
  it("normalizes supported browser locales", () => {
    expect(normalizeLocale("zh-TW")).toBe("zh-CN");
    expect(normalizeLocale("en-GB")).toBe("en-US");
  });

  it("keeps source text in Chinese mode", () => {
    expect(translateText("生态环境体检", "zh-CN")).toBe("生态环境体检");
  });

  it("uses curated product terminology", () => {
    expect(translateText("生态环境体检", "en-US")).toBe("Environment Check");
    expect(translateText("复制摘要给 AI", "en-US")).toBe("Copy Summary for AI");
  });

  it("translates dynamic messages without changing values", () => {
    expect(translateText("当前版本：1.2.3", "en-US")).toContain("1.2.3");
    expect(translateText("安装失败：network timeout", "en-US")).toBe("Installation failed: network timeout");
  });
});
