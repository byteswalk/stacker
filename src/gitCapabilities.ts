export type GitAccountCapability = {
  platform: string;
  provider?: string | null;
  service_name?: string | null;
};

const REMOTE_CREATE_PROVIDERS = new Set([
  "github",
  "gitee",
  "github-enterprise",
  "gitlab",
  "gitea",
  "forgejo",
]);

export function gitAccountProvider(account: GitAccountCapability) {
  return account.provider?.trim().toLowerCase() || account.platform.trim().toLowerCase();
}

export function supportsRemoteRepositoryCreation(account: GitAccountCapability) {
  return REMOTE_CREATE_PROVIDERS.has(gitAccountProvider(account));
}

export function remoteRepositoryCreationHint(account: GitAccountCapability) {
  const service = account.service_name?.trim() || "该 Git 服务";
  return `${service} 暂不支持自动创建远程仓库，请先在服务端创建仓库，再填写仓库的 HTTPS 地址。`;
}
