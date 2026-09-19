import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
// @ts-expect-error plain ESM script without types
import { extensionId } from "../scripts/extension-id.mjs";

const manifest = JSON.parse(readFileSync(new URL("../public/manifest.json", import.meta.url), "utf8"));

describe("manifest", () => {
  it("asks only for the supported sites and never for all URLs", () => {
    expect(manifest.host_permissions).toEqual(["https://chatgpt.com/*", "https://claude.ai/*"]);
    expect(JSON.stringify(manifest)).not.toContain("<all_urls>");
    expect(manifest.permissions).toEqual(["storage", "downloads"]);
  });
  it("has a fixed ID recorded in EXTENSION_ID", () => {
    const recorded = readFileSync(new URL("../EXTENSION_ID", import.meta.url), "utf8").trim();
    expect(recorded).toMatch(/^[a-p]{32}$/);
    expect(extensionId(manifest.key)).toBe(recorded);
  });
});
