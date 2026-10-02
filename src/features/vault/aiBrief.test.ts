import { describe, expect, it } from "vitest";
import type { EntryView } from "./api";
import { aiBrief, credentialCommand } from "./aiBrief";

const same = (text: string) => text;
const ssh: EntryView = {
  id: "k", title: "songgift_ed25519", platform: "gthost.com", kind: "ssh_key",
  fields: [
    { name: "私钥", secret: true, value: null, filled: true },
    { name: "公钥", secret: false, value: "ssh-ed25519 AAAA songgift", filled: true },
    { name: "口令", secret: true, value: null, filled: true },
    { name: "用途/主机", secret: false, value: "root@203.0.113.7:2222", filled: true },
    { name: "已装服务器", secret: false, value: "root@203.0.113.7", filled: true },
  ],
  expiresAt: "2026-11-02", tags: [], note: "", favorite: false, createdAt: 1, updatedAt: 2, deletedAt: null, historyCount: 0,
  ssh: { algorithm: "ssh-ed25519", bits: null, encrypted: true, fingerprint: "SHA256:abc", publicKey: "ssh-ed25519 AAAA", risks: [] },
};

describe("the text handed to an AI", () => {
  it("names an SSH key by how to connect with it, and never carries the key or its passphrase", () => {
    const text = aiBrief(ssh, { path: "C:\\Users\\me\\.ssh\\songgift_ed25519", alias: null }, same);
    expect(text).toContain('连接命令: ssh -i "C:\\Users\\me\\.ssh\\songgift_ed25519" -p 2222 root@203.0.113.7');
    expect(text).toContain("公钥: ssh-ed25519 AAAA songgift");
    expect(text).toContain("指纹: SHA256:abc");
    expect(text).toContain("私钥有口令");
    expect(text).not.toMatch(/^私钥:|^口令:/m);
    expect(aiBrief(ssh, { path: "x", alias: "songgift" }, same)).toContain("连接命令: ssh songgift\n");
    expect(aiBrief(ssh, { path: null, alias: null }, same)).toContain("私钥还没有放到本机");
  });

  it("lists another entry's plain values and only says a secret exists", () => {
    const token: EntryView = {
      ...ssh, kind: "api_key", title: "Ark", platform: "火山方舟", ssh: null, expiresAt: null,
      fields: [
        { name: "Key", secret: true, value: null, filled: true },
        { name: "Base URL", secret: false, value: "https://ark.example/v1", filled: true },
        { name: "套餐", secret: false, value: "", filled: false },
      ],
      note: " 绑定 work 邮箱 ",
    };
    const text = aiBrief(token, null, same);
    expect(text).toContain("Key: （保密，未包含：这个值只在 Stacker 保管库里");
    const held = aiBrief(token, null, same, [{ field: "Key", name: "ARK_API_KEY", scope: "user" }]);
    expect(held).toContain("Key: 在本机的用户环境变量里 ARK_API_KEY（PowerShell: $env:ARK_API_KEY · cmd: %ARK_API_KEY% · bash: $ARK_API_KEY）");
    expect(held).toContain("请在命令里直接引用上面的环境变量");
    const stored = aiBrief(token, null, same, [], [{ field: "Key", target: "Stacker:Ark:Key" }]);
    expect(stored).toContain("Key: 在 Windows 凭据管理器里（普通凭据），凭据名 Stacker:Ark:Key");
    expect(stored).toContain("CredReadW('Stacker:Ark:Key',1,0,[ref]$p)");
    expect(stored).toContain("不要把值打印出来");
    expect(stored).not.toContain("未包含");
    // A quote in a name cannot end the quoted PowerShell string.
    expect(credentialCommand("Stacker:it's:Key")).toContain("CredReadW('Stacker:it''s:Key',1,0");
    expect(text).toContain("Base URL: https://ark.example/v1");
    expect(text).toContain("备注: 绑定 work 邮箱");
    expect(text).not.toContain("套餐");
  });
});
