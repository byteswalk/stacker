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

// The app refuses to install an update it cannot check, so the manifest must carry the hashes
// that release-windows.ps1 writes, and SHA256SUMS.txt must be uploaded with the release.
for (const field of ["installer_sha256", "portable_sha256"]) {
  const value = latestMeta[field];
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/.test(value)) {
    throw new Error(`latest.json ${field} must be a lowercase 64-character SHA-256`);
  }
}

// Releases from here on are signed with the release key, and the app refuses to install an
// update it cannot verify. Earlier releases predate signing, so only newer ones are required
// to carry a signature.
const LAST_UNSIGNED_RELEASE = "0.3.3";
const asNumbers = (v) => v.split(".").map(Number);
const isAfter = (a, b) => {
  const [x, y] = [asNumbers(a), asNumbers(b)];
  for (let i = 0; i < Math.max(x.length, y.length); i += 1) {
    if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) > (y[i] ?? 0);
  }
  return false;
};

if (isAfter(version, LAST_UNSIGNED_RELEASE)) {
  const signature = latestMeta.installer_signature;
  if (typeof signature !== "string" || !signature.includes("untrusted comment:")) {
    throw new Error("latest.json installer_signature must hold the installer's minisign signature");
  }
}

console.log(`Release metadata is consistent for v${version}.`);
