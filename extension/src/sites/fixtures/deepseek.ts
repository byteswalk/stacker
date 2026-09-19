export const ok = (bizData: unknown) => ({ code: 0, msg: "", data: { biz_code: 0, biz_msg: "", biz_data: bizData } });

export const user = ok({ id: "ds-user-1", email: "someone@example.com", mobile_number: "13800000000", token: "tok-ds" });

export const pageFirst = ok({
  chat_sessions: [
    { id: "s-1", title: "Pinned plan", pinned: true, updated_at: 1_789_000_000.25, inserted_at: 1_788_000_000.5 },
    { id: "s-2", title: "Sorting", pinned: false, updated_at: 1_788_900_000.75, inserted_at: 1_788_800_000 },
  ],
  has_more: true,
});
/** The site's cursor is inclusive: the last session of the previous page comes back first. */
export const pageLast = ok({
  chat_sessions: [
    { id: "s-2", title: "Sorting", pinned: false, updated_at: 1_788_900_000.75, inserted_at: 1_788_800_000 },
    { id: "s-3", title: "", pinned: false, updated_at: 1_788_000_000, inserted_at: 1_787_000_000 },
  ],
  has_more: false,
});

/** Message 2 is an answer the user regenerated away from; message 4 is the one on screen. */
export const history = ok({
  chat_session: { id: "s-1", title: "Pinned plan", updated_at: 1_789_000_000.25, current_message_id: 4 },
  chat_messages: [
    { message_id: 1, parent_id: null, role: "USER", content: "Plan a sprint", inserted_at: 1_788_000_000.5, files: [{ file_name: "backlog.csv" }] },
    { message_id: 2, parent_id: 1, role: "ASSISTANT", content: "Old answer", inserted_at: 1_788_000_010 },
    { message_id: 3, parent_id: 1, role: "ASSISTANT", content: "", thinking_content: "hmm", inserted_at: 1_788_000_020, fragments: [{ type: "THINK", content: "hmm" }, { type: "RESPONSE", content: "Two weeks, three goals." }] },
    { message_id: 4, parent_id: 3, role: "USER", content: "Shorter?", inserted_at: 1_788_000_030 },
  ],
});

export const signedOut = { code: 40003, msg: "Authorization Failed (invalid token)", data: null };
export const serverError = { code: 50000, msg: "Internal error", data: null };
