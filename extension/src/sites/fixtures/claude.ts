export const orgs = [
  { uuid: "org-api", name: "API", capabilities: ["api"] },
  { uuid: "org-chat", name: "Personal", capabilities: ["chat", "claude_pro"] },
];
export const listFull = Array.from({ length: 100 }, (_, i) => ({ uuid: `k${i}`, name: `Chat ${i}`, created_at: "2026-09-01T00:00:00Z", updated_at: "2026-09-02T00:00:00Z" }));
export const listTail = [{ uuid: "k100", name: "", created_at: "2026-08-01T00:00:00Z", updated_at: "2026-08-02T00:00:00Z" }];
export const conversation = {
  uuid: "k1",
  name: "Refactor",
  updated_at: "2026-09-02T00:00:00Z",
  current_leaf_message_uuid: "m3",
  chat_messages: [
    { uuid: "m1", parent_message_uuid: "00000000-0000-4000-8000-000000000000", sender: "human", text: "Help me refactor", content: [{ type: "text", text: "Help me refactor" }], created_at: "2026-09-02T00:00:00Z", attachments: [{ file_name: "a.ts" }], files: [] },
    { uuid: "m2old", parent_message_uuid: "m1", sender: "assistant", content: [{ type: "text", text: "Old answer" }], created_at: "2026-09-02T00:00:01Z" },
    { uuid: "m3", parent_message_uuid: "m1", sender: "assistant", content: [{ type: "tool_use", name: "x" }, { type: "text", text: "Here is the plan." }], created_at: "2026-09-02T00:00:02Z" },
  ],
};
