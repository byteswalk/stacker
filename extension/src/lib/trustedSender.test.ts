import { describe, expect, it } from "vitest";
import { isTrustedSender } from "./trustedSender";

const ID = "abcdefghabcdefghabcdefghabcdefgh";
const URL = `chrome-extension://${ID}/`;

describe("isTrustedSender", () => {
  it("accepts the extension's own pages", () => {
    expect(isTrustedSender({ id: ID, url: `${URL}manage.html` }, ID, URL)).toBe(true);
    expect(isTrustedSender({ id: ID, url: `${URL}popup.html` }, ID, URL)).toBe(true);
  });

  it("rejects a content script: it shares the extension id but its url is the website's own", () => {
    expect(isTrustedSender({ id: ID, url: "https://chat.openai.com/" }, ID, URL)).toBe(false);
  });

  it("rejects a message claiming a different extension id", () => {
    expect(isTrustedSender({ id: "someone-else", url: `${URL}manage.html` }, ID, URL)).toBe(false);
  });

  it("rejects a sender with no url at all", () => {
    expect(isTrustedSender({ id: ID }, ID, URL)).toBe(false);
  });
});
