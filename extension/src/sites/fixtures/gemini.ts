/** Trimmed /app page: only the WIZ_global_data keys the adapter reads, plus the email it must never read. */
export const appHtml = '<!doctype html><html><head><script data-id="_gd" nonce="n">window.WIZ_global_data = {"oPEP7c":"someone@example.com","S06Grb":"108000000000000000001","SNlM0e":"AKlEn5_tok:1789000000000","cfb2h":"boq_assistant-bard-web-server_20260915.08_p0","FdrFJe":"-1234567890123456789","qwAQke":"BardChatUi"};</script></head><body></body></html>';

/** Signed-out /app page: no SNlM0e and no user id. */
export const signedOutHtml = '<!doctype html><html><head><script>window.WIZ_global_data = {"cfb2h":"boq_assistant-bard-web-server_20260915.08_p0","FdrFJe":"-1234567890123456789","qwAQke":"BardChatUi"};</script></head></html>';

/** A batchexecute reply as the site sends it: an anti-JSON prefix, then length-prefixed JSON lines. */
export function batchReply(rpcId: string, inner: unknown): string {
  const line = JSON.stringify([["wrb.fr", rpcId, JSON.stringify(inner), null, null, null, "generic"], ["di", 57], ["af.httprm", 56, "-3141592653589793238", 3]]);
  return `)]}'\n\n${line.length}\n${line}\n25\n[["e",4,null,null,${line.length + 30}]]\n`;
}

/** A batchexecute reply whose rpc failed: no result, a status array in position 5. */
export function errorReply(rpcId: string, code: number): string {
  const line = JSON.stringify([["wrb.fr", rpcId, null, null, null, [code], "generic"], ["di", 21]]);
  return `)]}'\n\n${line.length}\n${line}\n`;
}

/** MaZiqc results: a first page with a next-page token, then the last page. */
export const listFirst = [null, "tok-2", [
  ["c_aaa111", "Trip plan", null, null, null, [1_789_000_000, 500_000_000]],
  ["c_bbb222", "", null, null, null, [1_788_000_000, 0]],
]];
export const listLast = [null, null, [["c_ccc333", "Old chat", null, null, null, [1_787_000_000, 0]]]];

/** hNvQHb turns, newest first as Gemini sends them. */
export const chatNewestFirst: unknown[][] = [
  [["c_aaa111", "r_2"], ["c_aaa111", "r_2", "rc_2"], [["And in winter?"], 1, null, 0], [[["rc_2", ["Snowy and quiet."]]]], [1_789_000_100, 0]],
  [["c_aaa111", "r_1"], ["c_aaa111", "r_1", "rc_1"], [["Where to go in Kyoto?"], 1, null, 0], [[["rc_1", ["Try Arashiyama."]]]], [1_789_000_000, 0]],
];
