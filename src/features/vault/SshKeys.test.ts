import { describe, expect, it } from "vitest";
import type { EntryView } from "./api";
import { installCommand, keyFileName, parseTarget, publicKeyOf, serversOf, SERVERS_FIELD, withFields } from "./SshKeys";

const entry: EntryView = {
  id: "k1", title: "Hostinger VPS (新)", platform: "", kind: "ssh_key",
  fields: [
    { name: "私钥", secret: true, value: null, filled: true },
    { name: "公钥", secret: false, value: "ssh-ed25519 AAAA field", filled: true },
    { name: "用途/主机", secret: false, value: "root@1.2.3.4", filled: true },
  ],
  expiresAt: null, tags: ["vps"], note: "n", favorite: false,
  createdAt: 1, updatedAt: 2, deletedAt: null, historyCount: 0, ssh: null,
};

describe("ssh key helpers", () => {
  it("builds one command that appends the key and fixes the modes", () => {
    expect(installCommand(" ssh-ed25519 AAAA me@box\n")).toBe(
      "mkdir -p ~/.ssh && chmod 700 ~/.ssh && echo 'ssh-ed25519 AAAA me@box' >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys",
    );
    // A quote in an imported key's comment cannot end the quoted string early.
    expect(installCommand("ssh-rsa AAAA it's")).toContain(String.raw`echo 'ssh-rsa AAAA it'\''s' >>`);
  });

  it("turns a title into a file name and a target into its parts", () => {
    expect(keyFileName("Hostinger VPS (新)")).toBe("Hostinger_VPS");
    expect(keyFileName("  ")).toBe("id_stacker");
    expect(parseTarget("root@1.2.3.4:2222")).toEqual({ user: "root", host: "1.2.3.4", port: "2222" });
    expect(parseTarget("vps.example.com")).toEqual({ user: "", host: "vps.example.com", port: "" });
    expect(parseTarget("生产环境的那台")).toEqual({ user: "", host: "", port: "" });
  });

  it("prefers the key read from the private key over the stored field", () => {
    expect(publicKeyOf(entry)).toBe("ssh-ed25519 AAAA field");
    expect(publicKeyOf({ ...entry, ssh: { algorithm: "ssh-ed25519", bits: null, encrypted: false, fingerprint: null, publicKey: "ssh-ed25519 AAAA real", risks: [] } })).toBe("ssh-ed25519 AAAA real");
  });

  it("records servers on the entry without touching its secrets", () => {
    expect(serversOf(entry)).toEqual([]);
    const input = withFields(entry, { [SERVERS_FIELD]: "root@1.2.3.4\nroot@5.6.7.8" });
    expect(input.fields).toEqual([
      { name: "私钥", previousName: "私钥", secret: true, value: null },
      { name: "公钥", previousName: "公钥", secret: false, value: "ssh-ed25519 AAAA field" },
      { name: "用途/主机", previousName: "用途/主机", secret: false, value: "root@1.2.3.4" },
      { name: SERVERS_FIELD, previousName: null, secret: false, value: "root@1.2.3.4\nroot@5.6.7.8" },
    ]);
    expect(input.tags).toEqual(["vps"]);
    const saved = { ...entry, fields: [...entry.fields, { name: SERVERS_FIELD, secret: false, value: "a\n\n b ", filled: true }] };
    expect(serversOf(saved)).toEqual(["a", "b"]);
    expect(withFields(saved, { [SERVERS_FIELD]: "a" }).fields.at(-1)).toEqual({ name: SERVERS_FIELD, previousName: SERVERS_FIELD, secret: false, value: "a" });
  });
});
