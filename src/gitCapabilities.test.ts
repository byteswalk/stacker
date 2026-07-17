import { describe, expect, it } from "vitest";
import {
  gitAccountProvider,
  remoteRepositoryCreationHint,
  supportsRemoteRepositoryCreation,
} from "./gitCapabilities";

describe("Git account capabilities", () => {
  it.each(["github", "gitee", "github-enterprise", "gitlab", "gitea", "forgejo"])(
    "allows remote repository creation for %s",
    (provider) => {
      expect(supportsRemoteRepositoryCreation({ platform: "custom", provider })).toBe(true);
    },
  );

  it("requires an existing repository for Codeup", () => {
    const account = { platform: "custom", provider: "aliyun-codeup", service_name: "云效 Codeup" };
    expect(gitAccountProvider(account)).toBe("aliyun-codeup");
    expect(supportsRemoteRepositoryCreation(account)).toBe(false);
    expect(remoteRepositoryCreationHint(account)).toContain("填写仓库的 HTTPS 地址");
  });

  it("does not assume an unknown service supports repository creation", () => {
    expect(supportsRemoteRepositoryCreation({ platform: "custom", provider: "generic" })).toBe(false);
  });
});
