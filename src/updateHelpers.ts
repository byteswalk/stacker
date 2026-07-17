export type VersionUpdateInfo = {
  current: string;
  latest: string;
  has_update: boolean;
  source_name: string;
};

export type EcosystemVersionUpdate = {
  id: string;
  name: string;
  current: string;
  latest: string;
  source: string;
};

export function ecosystemUpdateFromInfo(
  id: string,
  name: string,
  info: VersionUpdateInfo | null,
): EcosystemVersionUpdate | null {
  if (!info?.has_update || !info.current.trim() || !info.latest.trim()) return null;
  return {
    id,
    name,
    current: info.current,
    latest: info.latest,
    source: info.source_name,
  };
}
