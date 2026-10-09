/**
 * The version as shown in the app, with the build it came from: `0.3.4 (r74)` for a numbered
 * release build (the release script passes `-Revision r74`), `0.3.4 (dev)` when run from
 * source. Builds of the same version are otherwise impossible to tell apart.
 */
export function versionLabel(version: string, revision = import.meta.env.VITE_STACKER_REVISION, dev = import.meta.env.DEV): string {
  if (!version) return version;
  const build = revision?.trim() || (dev ? "dev" : "");
  return build ? `${version} (${build})` : version;
}
