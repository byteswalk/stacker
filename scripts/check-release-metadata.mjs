import { readFile } from "node:fs/promises";

const readJson = async (path) => JSON.parse(await readFile(path, "utf8"));

const packageMeta = await readJson("package.json");
const tauriMeta = await readJson("src-tauri/tauri.conf.json");
const latestMeta = await readJson("resources/latest.json");
const cargoToml = await readFile("src-tauri/Cargo.toml", "utf8");
const cargoVersion = cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const version = packageMeta.version;

const versions = {
  "package.json": version,
  "tauri.conf.json": tauriMeta.version,
  "Cargo.toml": cargoVersion,
  "latest.json": latestMeta.version,
};

const mismatches = Object.entries(versions)
  .filter(([, candidate]) => candidate !== version)
  .map(([file, candidate]) => `${file}=${candidate ?? "missing"}`);

if (mismatches.length > 0) {
  throw new Error(`Release version mismatch: package.json=${version}; ${mismatches.join("; ")}`);
}

const expectedTag = `/v${version}`;
const expectedInstaller = `Stacker-${version}-setup-windows-x64.exe`;
const expectedPortable = `Stacker-${version}-portable-windows-x64.zip`;
const urlChecks = [
  ["release_url", latestMeta.release_url, expectedTag],
  ["installer_url", latestMeta.installer_url, `${expectedTag}/${expectedInstaller}`],
  ["portable_url", latestMeta.portable_url, `${expectedTag}/${expectedPortable}`],
];

for (const [field, value, expected] of urlChecks) {
  if (typeof value !== "string" || !value.includes(expected)) {
    throw new Error(`latest.json ${field} must contain ${expected}`);
  }
}

console.log(`Release metadata is consistent for v${version}.`);
