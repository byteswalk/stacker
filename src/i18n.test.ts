import { describe, expect, it } from "vitest";
import { normalizeLocale, translateText } from "./i18n";

describe("internationalization", () => {
  it("normalizes supported browser locales", () => {
    expect(normalizeLocale("zh-TW")).toBe("zh-CN");
    expect(normalizeLocale("en-GB")).toBe("en-US");
  });

  it("keeps source text in Chinese mode", () => {
    expect(translateText("编程生态体检", "zh-CN")).toBe("编程生态体检");
  });

  it("uses curated product terminology", () => {
    expect(translateText("编程生态体检", "en-US")).toBe("Programming Ecosystem Check");
    expect(translateText("待体检", "en-US")).toBe("Pending");
    expect(translateText("体检中…", "en-US")).toBe("Checking...");
    expect(translateText("复制摘要给 AI", "en-US")).toBe("Copy Summary for AI");
    expect(translateText("快速扫描 · 选择目录 · 选择磁盘 · 全盘分析", "en-US"))
      .toBe("Quick Scan · Choose Folder · Choose Disk · All-disk Analysis");
  });

  it("keeps short action words imperative and status words past tense", () => {
    expect(translateText("删除", "en-US")).toBe("Delete");
    expect(translateText("修改", "en-US")).toBe("Modify");
    expect(translateText("显示", "en-US")).toBe("Show");
    expect(translateText("导入", "en-US")).toBe("Import");
    expect(translateText("已删除", "en-US")).toBe("Deleted");
    expect(translateText("已修改", "en-US")).toBe("Modified");
    expect(translateText("当前显示 ", "en-US")).toBe("Showing ");
  });

  it("translates dynamic messages without changing values", () => {
    expect(translateText("当前版本：1.2.3", "en-US")).toContain("1.2.3");
    expect(translateText("安装失败：network timeout", "en-US")).toBe("Installation failed: network timeout");
  });
});
