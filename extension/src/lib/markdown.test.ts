import { describe, expect, it } from "vitest";
import type { Conversation, StoredBody } from "./db";
import { exportFileName, toMarkdown } from "./markdown";

const conv: Conversation = {
  key: "chatgpt:abcdef123456", site: "chatgpt", account: "chatgpt:u", id: "abcdef123456", title: "Kyoto: plan/2",
  createdAt: Date.UTC(2026, 8, 1), updatedAt: Date.UTC(2026, 8, 2), archived: false,
  folderId: null, tags: ["travel"], favorite: false, note: "", bodyFetchedAt: 1, bodyUpdatedAt: 1, removedAt: null,
  listedAt: 1, localUpdatedAt: 0,
};
const body: StoredBody = {
  key: conv.key, id: conv.id, title: conv.title, updatedAt: 1,
  messages: [
    { role: "user", text: "Where?", at: Date.UTC(2026, 8, 1, 1), attachments: ["map.png"] },
    { role: "tool", text: "print(1)", at: null, attachments: [] },
    { role: "assistant", text: "Kyoto.", at: null, attachments: [] },
  ],
};

describe("markdown export", () => {
  it("slim keeps only user and assistant text under a header", () => {
    const md = toMarkdown(conv, "Work", body, "slim", "https://chatgpt.com/c/abcdef123456");
    expect(md).toContain("# Kyoto: plan/2");
    expect(md).toContain("- 站点：ChatGPT");
    expect(md).toContain("- 账号：Work");
    expect(md).toContain("- 链接：https://chatgpt.com/c/abcdef123456");
    expect(md).toContain("## 用户\n\nWhere?");
    expect(md).toContain("## 助手\n\nKyoto.");
    expect(md).not.toContain("print(1)");
    expect(md).not.toContain("map.png");
  });
  it("full adds tool messages, attachments and times", () => {
    const md = toMarkdown(conv, "Work", body, "full", "u");
    expect(md).toContain("## 工具");
    expect(md).toContain("附件：map.png");
    expect(md).toContain("2026-09-01");
  });
  it("makes a safe, dated file name per site", () => {
    expect(exportFileName(conv, "md")).toBe("chatgpt/2026-09-02 Kyoto plan 2 abcdef12.md");
  });
});
