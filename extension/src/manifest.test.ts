import { existsSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
// @ts-expect-error plain ESM script without types
import { extensionId } from "../scripts/extension-id.mjs";
import { SITES } from "./sites/registry";

const manifest = JSON.parse(readFileSync(new URL("../public/manifest.json", import.meta.url), "utf8"));
const SITE_MATCHES = ["https://chatgpt.com/*", "https://claude.ai/*", "https://gemini.google.com/*", "https://grok.com/*", "https://chat.deepseek.com/*"];

describe("manifest", () => {
  it("asks only for the five supported sites and never for all URLs", () => {
    expect(manifest.host_permissions).toEqual(SITE_MATCHES);
    expect(manifest.content_scripts).toEqual([{ matches: SITE_MATCHES, js: ["content.js"], run_at: "document_idle" }]);
    expect(JSON.stringify(manifest)).not.toContain("<all_urls>");
    expect(manifest.permissions).toEqual(["storage", "downloads", "nativeMessaging"]);
  });
  it("covers exactly the sites in the registry", () => {
    expect(Object.values(SITES).map((s) => s.match)).toEqual(SITE_MATCHES);
  });
  it("ships Stacker's icon in every size it declares", () => {
    const sizes = ["16", "32", "48", "128"];
    expect(Object.keys(manifest.icons)).toEqual(sizes);
    expect(Object.keys(manifest.action.default_icon)).toEqual(sizes);
    for (const path of Object.values<string>({ ...manifest.icons, ...manifest.action.default_icon })) {
      expect(existsSync(new URL(`../public/${path}`, import.meta.url))).toBe(true);
    }
  });
  it("has a fixed ID recorded in EXTENSION_ID", () => {
    const recorded = readFileSync(new URL("../EXTENSION_ID", import.meta.url), "utf8").trim();
    expect(recorded).toMatch(/^[a-p]{32}$/);
    expect(extensionId(manifest.key)).toBe(recorded);
  });
});
