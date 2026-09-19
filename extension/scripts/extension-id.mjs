import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

/** Chrome's extension ID: first 32 hex digits of SHA-256(public key DER), 0-f mapped to a-p. */
export function extensionId(keyBase64) {
  const hex = createHash("sha256").update(Buffer.from(keyBase64, "base64")).digest("hex").slice(0, 32);
  return [...hex].map((c) => String.fromCharCode(97 + parseInt(c, 16))).join("");
}

if (process.argv[1] && process.argv[1].endsWith("extension-id.mjs")) {
  const manifest = JSON.parse(readFileSync(new URL("../public/manifest.json", import.meta.url), "utf8"));
  console.log(extensionId(manifest.key));
}
