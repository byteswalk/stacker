import { describe, expect, it } from "vitest";
import { normalizeLocale, translateText } from "./i18n";

describe("internationalization", () => {
  it("normalizes supported browser locales", () => {
    expect(normalizeLocale("zh-TW")).toBe("zh-CN");
    expect(normalizeLocale("en-GB")).toBe("en-US");
  });

  it("replaces a one-character word only when it stands alone", () => {
    // The 中 of 中文 is not "Medium".
    expect(translateText("中文 / English", "en-US")).toBe("中文 / English");
    expect(translateText("中", "en-US")).toBe("Medium");
    expect(translateText("共 3 项", "en-US")).not.toMatch(/[㐀-鿿]/);
  });

  it("keeps a space between words translated side by side", () => {
    expect(translateText("Maven下载源", "en-US")).toBe("Maven Download Source");
    for (const text of ["全部智能体", "终端环境变量", "共 3 项"]) {
      const out = translateText(text, "en-US");
      expect(out).not.toMatch(/[a-z][A-Z]/);
      expect(out.includes(String.fromCharCode(0xe000)) || out.includes(String.fromCharCode(0xe001))).toBe(false);
    }
  });

  it("finds a multi-line tip listed with its line breaks folded", () => {
    const tip = [
      "Python 命令通过 PATH 生效，新终端会自动使用当前默认版本。",
      "绿色终端按钮会在 Stacker 目录打开对应终端，可运行 python -V 验证版本。",
      "py 是 Windows Python Launcher，不代表当前默认版本。",
      "终端中找不到 python 时，可点击「更新集成」刷新用户 PATH。",
    ].join("\n");
    expect(translateText(tip, "en-US")).toMatch(/^Python commands take effect through PATH/);
    expect(translateText("共 18 条", "en-US")).toContain("18 items");
  });

  it("keeps source text in Chinese mode", () => {
    expect(translateText("环境总览", "zh-CN")).toBe("环境总览");
  });

  it("uses curated product terminology", () => {
    expect(translateText("环境总览", "en-US")).toBe("Overview");
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
