import type { SnapshotMetadata } from "./types";

export function latestComparablePair(items: readonly SnapshotMetadata[], fingerprint: string) {
  const matching = items.filter((item) => item.targetFingerprint === fingerprint)
    .sort((left, right) => right.createdAt.localeCompare(left.createdAt));
  return matching.length >= 2 ? { current: matching[0], base: matching[1] } : null;
}

export function signedBytes(value: number, format: (bytes: number) => string) {
  if (value === 0) return format(0);
  return `${value > 0 ? "+" : "-"}${format(Math.abs(value))}`;
}
