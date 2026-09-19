export const session = { user: { id: "user-abc", email: "someone@example.com" }, accessToken: "tok" };
export const listPage = {
  items: [
    { id: "c1", title: "Plan a trip", create_time: "2026-09-01T10:00:00Z", update_time: "2026-09-02T10:00:00Z", is_archived: false },
    { id: "c2", title: "Fix build", create_time: 1_788_000_000, update_time: 1_788_100_000.5 },
  ],
  total: 2, limit: 100, offset: 0,
};
export const conversation = {
  title: "Plan a trip",
  update_time: 1_788_200_000,
  current_node: "n4",
  mapping: {
    root: { id: "root", parent: null, message: null },
    n1: { id: "n1", parent: "root", message: { author: { role: "system" }, create_time: null, content: { content_type: "text", parts: [""] }, metadata: { is_visually_hidden_from_conversation: true } } },
    n2: { id: "n2", parent: "n1", message: { author: { role: "user" }, create_time: 1_788_000_000, content: { content_type: "text", parts: ["Where to go?"] }, metadata: { attachments: [{ name: "map.png" }] } } },
    n3old: { id: "n3old", parent: "n2", message: { author: { role: "assistant" }, create_time: 1_788_000_010, content: { content_type: "text", parts: ["Old branch"] } } },
    n3: { id: "n3", parent: "n2", message: { author: { role: "assistant" }, create_time: 1_788_000_020, content: { content_type: "text", parts: ["Try Kyoto."] } } },
    n4: { id: "n4", parent: "n3", message: { author: { role: "tool" }, create_time: 1_788_000_030, content: { content_type: "code", text: "print(1)" } } },
  },
};
