import type { Role } from "../shared/types";
import { SITES } from "../sites/registry";
import type { Conversation, StoredBody } from "./db";

export type ExportMode = "slim" | "full";

const ROLE_LABEL: Record<Role, string> = { user: "用户", assistant: "助手", system: "系统", tool: "工具" };
const day = (ms: number) => new Date(ms).toISOString().slice(0, 10);
const stamp = (ms: number) => new Date(ms).toISOString().replace("T", " ").slice(0, 16);

export function toMarkdown(c: Conversation, accountAlias: string, body: StoredBody, mode: ExportMode, url: string): string {
  const head = [
    `# ${c.title || c.id}`,
    "",
    `- 站点：${SITES[c.site].label}`,
    `- 账号：${accountAlias}`,
    `- 链接：${url}`,
    `- 创建：${stamp(c.createdAt)}`,
    `- 更新：${stamp(c.updatedAt)}`,
    ...(c.tags.length ? [`- 标签：${c.tags.join("、")}`] : []),
    ...(c.note ? [`- 备注：${c.note}`] : []),
  ];
  const messages = body.messages
    .filter((m) => mode === "full" || m.role === "user" || m.role === "assistant")
    .map((m) => {
      const title = `## ${ROLE_LABEL[m.role]}${mode === "full" && m.at !== null ? ` · ${stamp(m.at)}` : ""}`;
      const files = mode === "full" && m.attachments.length ? `\n\n附件：${m.attachments.join("、")}` : "";
      return `${title}\n\n${m.text}${files}`;
    });
  return [...head, "", ...messages.flatMap((m) => [m, ""])].join("\n");
}

export function exportFileName(c: Conversation, ext: "md" | "json"): string {
  const title = (c.title || "untitled").replace(/[\\/:*?"<>|\s]+/g, " ").trim().slice(0, 60);
  return `${c.site}/${day(c.updatedAt)} ${title} ${c.id.slice(0, 8)}.${ext}`;
}
