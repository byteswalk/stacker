export const listFirst = {
  conversations: [
    { conversationId: "g-1", title: "Rust lifetimes", starred: false, createTime: "2026-09-01T10:00:00.000Z", modifyTime: "2026-09-02T10:00:00.000Z" },
    { conversationId: "g-2", title: "", starred: true, createTime: "2026-08-01T10:00:00.000Z" },
  ],
  nextPageToken: "p2",
};
export const listLast = {
  conversations: [{ conversationId: "g-3", title: "Old", starred: false, createTime: "2026-07-01T10:00:00.000Z", modifyTime: "2026-07-01T11:00:00.000Z" }],
};
/** r2old is an answer the user regenerated away from; the last node (r4) is the one on screen. */
export const responseNodes = {
  responseNodes: [
    { responseId: "r1", sender: "human" },
    { responseId: "r2old", sender: "assistant", parentResponseId: "r1" },
    { responseId: "r2", sender: "assistant", parentResponseId: "r1" },
    { responseId: "r3", sender: "human", parentResponseId: "r2" },
    { responseId: "r4", sender: "assistant", parentResponseId: "r3" },
  ],
};
export const responses = {
  responses: [
    { responseId: "r1", message: "Explain lifetimes", sender: "human", createTime: "2026-09-01T10:00:00.000Z", fileAttachments: [{ fileName: "main.rs" }] },
    { responseId: "r2", message: "A lifetime is a scope.", sender: "ASSISTANT", createTime: "2026-09-01T10:00:05.000Z", parentResponseId: "r1" },
    { responseId: "r3", message: "Example?", sender: "human", createTime: "2026-09-01T10:01:00.000Z", parentResponseId: "r2" },
    { responseId: "r4", message: "fn f<'a>(x: &'a str) {}", sender: "assistant", createTime: "2026-09-01T10:01:05.000Z", parentResponseId: "r3" },
  ],
};
