/**
 * The numbers of a version, ready to compare: `v22.11.0` → [22, 11, 0], `go1.23.4` → [1, 23, 4],
 * and Java's old `1.8.0_412` → [8, 0, 412], so JDK 8 sorts below 11.
 */
export function versionNumbers(version: string): number[] {
  const nums = (version.match(/\d+/g) ?? []).map(Number);
  if (/^(jdk-?)?1\.[5-8]\b/i.test(version.trim()) && nums.length > 1) return nums.slice(1);
  return nums;
}

/** Compares two versions number by number; a version with no numbers sorts below any with. */
export function compareVersions(a: string, b: string): number {
  const x = versionNumbers(a);
  const y = versionNumbers(b);
  if (!x.length || !y.length) return x.length - y.length;
  for (let i = 0; i < Math.max(x.length, y.length); i += 1) {
    const d = (x[i] ?? 0) - (y[i] ?? 0);
    if (d) return d;
  }
  return 0;
}

/** The list with the newest version on top, the same on every tool page. */
export function newestFirst<T>(items: readonly T[], version: (item: T) => string): T[] {
  return [...items].sort((a, b) => compareVersions(version(b), version(a)));
}

/** Rust's toolchains: the channels first (stable, beta, nightly), then numbered ones newest first. */
export function rustToolchainOrder<T>(items: readonly T[], name: (item: T) => string): T[] {
  const channel = (value: string) => ["stable", "beta", "nightly"].findIndex((c) => value.startsWith(c));
  return [...items].sort((a, b) => {
    const [x, y] = [channel(name(a)), channel(name(b))];
    if (x !== -1 || y !== -1) return (x === -1 ? 9 : x) - (y === -1 ? 9 : y);
    return compareVersions(name(b), name(a));
  });
}
