import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { EN } from "./i18n";

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return name === "fixtures" ? [] : files(path);
    return /\.(ts|tsx)$/.test(name) && !name.endsWith(".test.ts") && !name.endsWith(".test.tsx") ? [path] : [];
  });
}

describe("i18n", () => {
  it("has an English entry for every t() literal", () => {
    const root = new URL(".", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
    const missing = files(root).flatMap((file) =>
      [...readFileSync(file, "utf8").matchAll(/\bt\("([^"]+)"\)/g)].map((m) => m[1]).filter((text) => !(text in EN)));
    expect(missing).toEqual([]);
  });
});
