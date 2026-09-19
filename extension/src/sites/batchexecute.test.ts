import { describe, expect, it } from "vitest";
import { batchForm, batchUrl, extractTokens, parseBatch } from "./batchexecute";
import { appHtml, batchReply, errorReply, signedOutHtml } from "./fixtures/gemini";

const tokens = { at: "AKlEn5_tok:1789000000000", bl: "boq_assistant-bard-web-server_20260915.08_p0", fsid: "-1234567890123456789", userId: "108000000000000000001" };

describe("gemini batchexecute", () => {
  it("reads the page tokens and the user id from the /app HTML, never the email", () => {
    const got = extractTokens(appHtml);
    expect(got).toEqual(tokens);
    expect(JSON.stringify(got)).not.toContain("example.com");
  });

  it("decodes JSON escapes inside a token", () => {
    expect(extractTokens(appHtml.replace("AKlEn5_tok:1789000000000", "AKlEn5\\u003dtok")).at).toBe("AKlEn5=tok");
  });

  it("treats a page without the sign-in token as signed out, and other missing keys as a changed page", () => {
    expect(() => extractTokens(signedOutHtml)).toThrow("E_AUTH");
    expect(() => extractTokens(appHtml.replace('"cfb2h"', '"cfb2x"'))).toThrow("E_BROKEN: cfb2h");
    expect(() => extractTokens(appHtml.replace('"FdrFJe"', '"FdrFJx"'))).toThrow("E_BROKEN: FdrFJe");
    expect(() => extractTokens(appHtml.replace('"S06Grb"', '"S06Grx"'))).toThrow("E_BROKEN: S06Grb");
  });

  it("builds the request URL and form", () => {
    const url = new URL(batchUrl("MaZiqc", tokens, 12345));
    expect(url.origin + url.pathname).toBe("https://gemini.google.com/_/BardChatUi/data/batchexecute");
    expect(Object.fromEntries(url.searchParams)).toEqual({ rpcids: "MaZiqc", "source-path": "/app", bl: tokens.bl, "f.sid": tokens.fsid, hl: "en", _reqid: "12345", rt: "c" });
    const form = batchForm("MaZiqc", [100, null, [0, null, 1]], tokens);
    expect(form.at).toBe(tokens.at);
    expect(JSON.parse(form["f.req"])).toEqual([[["MaZiqc", "[100,null,[0,null,1]]", null, "generic"]]]);
  });

  it("finds the rpc's result among the reply lines", () => {
    expect(parseBatch(batchReply("MaZiqc", [null, "t", []]), "MaZiqc")).toEqual([null, "t", []]);
    expect(parseBatch(batchReply("GzXR5e", null), "GzXR5e")).toBeNull();
  });

  it("reports an rpc error as E_HTTP and a missing or unreadable reply as a changed interface", () => {
    expect(() => parseBatch(errorReply("GzXR5e", 3), "GzXR5e")).toThrow("E_HTTP");
    expect(() => parseBatch(batchReply("MaZiqc", []), "hNvQHb")).toThrow("E_BROKEN");
    expect(() => parseBatch("<html>sign in</html>", "MaZiqc")).toThrow("E_BROKEN");
  });
});
