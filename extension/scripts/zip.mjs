import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const { version } = JSON.parse(readFileSync(join(root, "dist", "manifest.json"), "utf8"));
const out = join(root, `stacker-web-chats-${version}.zip`);
execFileSync("powershell.exe", ["-NoProfile", "-Command", `Compress-Archive -Path '${join(root, "dist", "*")}' -DestinationPath '${out}' -Force`], { stdio: "inherit" });
console.log(out);
