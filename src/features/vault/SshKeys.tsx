import { useState } from "react";
import { Modal, useToast } from "../../ui";
import { vaultApi, vaultError, type EntryInput, type EntryView } from "./api";

/** The field of an SSH entry that lists the servers its public key was put on, one per line. */
export const SERVERS_FIELD = "已装服务器";
/** Where earlier versions kept one host; read when an entry still has it. */
const HOST_FIELD = "用途/主机";
const PASSPHRASE_FIELD = "口令";

export function publicKeyOf(entry: EntryView): string {
  // The key read from the private key is the one to trust; the stored line is used when it is
  // that same key with its comment, which an encrypted private key does not give away.
  const read = (entry.ssh?.publicKey ?? "").trim();
  const stored = (entry.fields.find((field) => field.name === "公钥")?.value ?? "").trim();
  return read && !stored.startsWith(read) ? read : stored || read;
}

export function serversOf(entry: EntryView): string[] {
  const text = entry.fields.find((field) => field.name === SERVERS_FIELD)?.value ?? "";
  return text.split("\n").map((line) => line.trim()).filter(Boolean);
}

/** The server to suggest when one is needed: the first on the list, or the host an older entry named. */
export function firstServer(entry: EntryView): string {
  return serversOf(entry)[0] ?? entry.fields.find((field) => field.name === HOST_FIELD)?.value?.trim() ?? "";
}

/** One line to run on the server: adds the key to `authorized_keys` and sets the modes sshd insists on. */
export function installCommand(publicKey: string): string {
  const quoted = publicKey.trim().replace(/'/g, String.raw`'\''`);
  return `mkdir -p ~/.ssh && chmod 700 ~/.ssh && echo '${quoted}' >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys`;
}

/** A name ssh and the file system both take: letters, digits, dot, dash, underscore. */
export function keyFileName(title: string): string {
  const name = title.trim().replace(/[^A-Za-z0-9._-]+/g, "_").replace(/^[._]+|[._]+$/g, "");
  return name || "id_stacker";
}

/** `root@1.2.3.4:2222` taken apart; whatever is missing stays empty. */
export function parseTarget(text: string): { user: string; host: string; port: string } {
  const match = /^\s*(?:([A-Za-z0-9._-]+)@)?([A-Za-z0-9._-]+)(?::(\d{1,5}))?\s*$/.exec(text);
  return match ? { user: match[1] ?? "", host: match[2], port: match[3] ?? "" } : { user: "", host: "", port: "" };
}

/** The entry as it is, with some plain fields set; secrets are left as they are on the backend. */
export function withFields(entry: EntryView, values: Record<string, string>): EntryInput {
  const known = new Set(entry.fields.map((field) => field.name));
  return {
    id: entry.id, title: entry.title, platform: entry.platform, kind: entry.kind,
    fields: [
      ...entry.fields.map((field) => ({
        name: field.name, previousName: field.name, secret: field.secret,
        value: field.name in values ? values[field.name] : field.secret ? null : field.value ?? "",
      })),
      ...Object.entries(values).filter(([name]) => !known.has(name)).map(([name, value]) => ({ name, previousName: null, value, secret: false })),
    ],
    expiresAt: entry.expiresAt, tags: entry.tags, note: entry.note, favorite: entry.favorite,
  };
}

async function copyPlain(text: string, done: string, toast: ReturnType<typeof useToast>) {
  try { await navigator.clipboard.writeText(text); toast(done, "ok"); }
  catch { toast("复制失败，请手动选中复制。", "err"); }
}

/** What a public key is for: hand it to a server, or put the pair where the local ssh finds it. */
export function SshKeyActions({ entry, onInstalled, onChanged }: {
  entry: EntryView; onInstalled?: () => void;
  /** The entry as saved after its passphrase changed. */
  onChanged?: (entry: EntryView) => void;
}) {
  const toast = useToast();
  const [installing, setInstalling] = useState(false);
  const [locking, setLocking] = useState(false);
  const encrypted = entry.ssh?.encrypted === true;
  const publicKey = publicKeyOf(entry);
  return <div className="vault-ssh-actions">
    <button className="gh sm" disabled={!publicKey} title="一行公钥，粘到 VPS 面板的 SSH Key 里" onClick={() => void copyPlain(publicKey, "已复制公钥。", toast)}>
      <i className="ti ti-copy" /> 复制公钥
    </button>
    <button className="gh sm" disabled={!publicKey} title="在服务器上执行这条命令，公钥就加进 authorized_keys 了" onClick={() => void copyPlain(installCommand(publicKey), "已复制安装命令，到服务器上粘贴执行。", toast)}>
      <i className="ti ti-terminal-2" /> 复制安装命令
    </button>
    <button className="gh sm" title="把私钥写到本机 ~/.ssh，可同时写好 ssh config" onClick={() => setInstalling(true)}>
      <i className="ti ti-device-desktop-down" /> 放到本机 ~/.ssh
    </button>
    {entry.ssh && <button className="gh sm" title="给私钥加一层口令：文件被拿走也用不了，连接时要输入口令" onClick={() => setLocking(true)}>
      <i className={"ti " + (encrypted ? "ti-lock-cog" : "ti-lock-plus")} /> {encrypted ? "修改口令" : "设置口令"}
    </button>}
    {locking && <SshPassphrase entry={entry} onClose={() => setLocking(false)} onDone={(saved) => { setLocking(false); onChanged?.(saved); }} />}
    {installing && <SshLocalInstall entry={entry} onClose={() => setInstalling(false)} onDone={() => { setInstalling(false); onInstalled?.(); }} />}
  </div>;
}

/** The private key's own passphrase: set on a key without one, changed, or taken off. */
export function SshPassphrase({ entry, onClose, onDone }: { entry: EntryView; onClose: () => void; onDone: (entry: EntryView) => void }) {
  const toast = useToast();
  const encrypted = entry.ssh?.encrypted === true;
  const recorded = entry.fields.some((field) => field.name === PASSPHRASE_FIELD && field.secret && field.filled);
  const [old, setOld] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const removing = encrypted && next === "" && confirm === "";
  const ready = next === confirm && (encrypted || next !== "") && (!encrypted || recorded || old !== "");

  async function submit() {
    setBusy(true);
    try {
      const saved = await vaultApi.sshSetPassphrase(entry.id, old, next, PASSPHRASE_FIELD);
      toast(next ? "口令已设置，新口令记在条目的「口令」里。" : "口令已移除。", "ok");
      onDone(saved);
    } catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }

  return <Modal title={encrypted ? "修改口令" : "设置口令"} icon="ti-lock" onClose={busy ? undefined : onClose}
    sub="口令加密的是私钥本身：放到本机或导出的私钥文件被拿走也用不了，连接时 ssh 会让你输入口令。"
    footer={<><button className="gh sm" disabled={busy} onClick={onClose}>取消</button>
      <button className={"pr sm"} style={removing ? { background: "#d6463d" } : undefined} disabled={busy || !ready} onClick={() => void submit()}>{removing ? "移除口令" : "保存"}</button></>}>
    <form className="vault-form" onSubmit={(event) => { event.preventDefault(); if (ready && !busy) void submit(); }}>
      {encrypted && <label>当前口令<input className="ip full" type="password" autoComplete="off" autoFocus value={old}
        placeholder={recorded ? "留空则使用条目里记录的口令" : ""} onChange={(e) => setOld(e.target.value)} /></label>}
      <label>新口令<input className="ip full" type="password" autoComplete="new-password" autoFocus={!encrypted} value={next}
        placeholder={encrypted ? "留空则移除口令" : ""} onChange={(e) => setNext(e.target.value)} /></label>
      <label>再输一次<input className="ip full" type="password" autoComplete="new-password" value={confirm} onChange={(e) => setConfirm(e.target.value)} /></label>
      {next !== confirm && confirm !== "" && <div className="vault-warn">两次输入的口令不一致。</div>}
      {removing && <div className="vault-warn">不设口令时，拿到私钥文件的人可以直接用它登录。</div>}
      <div className="vault-sub" style={{ margin: 0 }}>已经放到本机 ~/.ssh 的私钥文件不会跟着变，需要的话删掉旧文件再放一次。</div>
    </form>
  </Modal>;
}

/** Writes the private key to `~/.ssh` and, when asked, a `Host` block to the config: only on this click, never over a file. */
export function SshLocalInstall({ entry, onClose, onDone }: { entry: EntryView; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const target = parseTarget(firstServer(entry));
  const [name, setName] = useState(() => keyFileName(entry.title));
  const [config, setConfig] = useState(target.host !== "");
  const [alias, setAlias] = useState(() => keyFileName(entry.title));
  const [host, setHost] = useState(target.host);
  const [user, setUser] = useState(target.user || "root");
  const [port, setPort] = useState(target.port || "22");
  const [busy, setBusy] = useState(false);
  const portNumber = Number(port);
  const ready = name.trim() !== "" && (!config || (alias.trim() !== "" && host.trim() !== "" && user.trim() !== "" && Number.isInteger(portNumber) && portNumber > 0 && portNumber < 65536));

  async function submit() {
    setBusy(true);
    try {
      const path = await vaultApi.sshInstallLocal(entry.id, name.trim(), config ? { alias: alias.trim(), host: host.trim(), user: user.trim(), port: portNumber } : null);
      toast(config ? `私钥已写到 ${path}。连接命令：ssh ${alias.trim()}` : `私钥已写到 ${path}`, "ok");
      onDone();
    } catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }

  return <Modal title="放到本机 ~/.ssh" icon="ti-device-desktop-down" onClose={busy ? undefined : onClose}
    sub="只新增文件、追加配置，不覆盖已有内容；改 config 前会自动备份。"
    footer={<><button className="gh sm" disabled={busy} onClick={onClose}>取消</button><button className="pr sm" disabled={busy || !ready} onClick={() => void submit()}>写入</button></>}>
    <div className="vault-form">
      <label>私钥文件名<input className="ip full" value={name} spellCheck={false} onChange={(e) => setName(e.target.value)} /></label>
      <div className="vault-sub" style={{ margin: 0 }} translate="no">~/.ssh/{name || "…"}</div>
      <label style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
        <input type="checkbox" checked={config} onChange={(e) => setConfig(e.target.checked)} /> 同时写入 ~/.ssh/config，之后用别名直接连接
      </label>
      {config && <div className="vault-ssh-host">
        <label>别名<input className="ip full" value={alias} spellCheck={false} onChange={(e) => setAlias(e.target.value)} /></label>
        <label>主机<input className="ip full" value={host} spellCheck={false} placeholder="IP 或域名" onChange={(e) => setHost(e.target.value)} /></label>
        <label>用户<input className="ip full" value={user} spellCheck={false} onChange={(e) => setUser(e.target.value)} /></label>
        <label>端口<input className="ip full" value={port} inputMode="numeric" onChange={(e) => setPort(e.target.value.replace(/\D/g, ""))} /></label>
      </div>}
    </div>
  </Modal>;
}

/** Where the public key was installed, kept on the entry: the list to walk when the key is replaced. */
export function SshServers({ entry, onChanged }: { entry: EntryView; onChanged: () => void }) {
  const toast = useToast();
  const [adding, setAdding] = useState("");
  const [busy, setBusy] = useState(false);
  const servers = serversOf(entry);

  async function save(next: string[]) {
    setBusy(true);
    try { await vaultApi.save(withFields(entry, { [SERVERS_FIELD]: next.join("\n") })); setAdding(""); onChanged(); }
    catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }
  const add = () => { const value = adding.trim(); if (value && !servers.includes(value)) void save([...servers, value]); };

  return <div className="vault-field vault-servers">
    <span className="name">已装服务器</span>
    <div>
      {servers.length > 0 && <div className="vault-tags">{servers.map((server) => (
        <span className="vault-badge" key={server} translate="no">{server}
          <button type="button" title="移除" aria-label="移除" disabled={busy} onClick={() => void save(servers.filter((item) => item !== server))}><i className="ti ti-x" /></button>
        </span>
      ))}</div>}
      <form className="vault-servers-add" onSubmit={(event) => { event.preventDefault(); add(); }}>
        <input className="ip" value={adding} disabled={busy} spellCheck={false} placeholder="例如 root@1.2.3.4" onChange={(e) => setAdding(e.target.value)} />
        <button type="submit" className="gh sm" disabled={busy || !adding.trim()}><i className="ti ti-plus" /> 记一台</button>
      </form>
      <div className="vault-sub" style={{ margin: "4px 0 0" }}>公钥装到了哪台服务器就记在这里，以后换密钥时知道要去哪几台撤销。</div>
    </div>
    <span />
  </div>;
}

type Algorithm = "ed25519" | "rsa";

/** A new key pair made inside the vault: named, saved as an entry, then handed out as a public key. */
export function SshKeyGenerator({ onClose, onSaved }: { onClose: () => void; onSaved: (entry: EntryView) => void }) {
  const toast = useToast();
  const [title, setTitle] = useState("");
  const [algorithm, setAlgorithm] = useState<Algorithm>("ed25519");
  const [comment, setComment] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [again, setAgain] = useState("");
  const [busy, setBusy] = useState(false);
  const [entry, setEntry] = useState<EntryView | null>(null);

  async function generate() {
    setBusy(true);
    try {
      const name = title.trim();
      const pair = await vaultApi.sshGenerate(algorithm, comment.trim() || keyFileName(name), passphrase);
      const saved = await vaultApi.save({
        id: null, title: name, platform: "", kind: "ssh_key",
        fields: [
          { name: "私钥", previousName: null, value: pair.privateKey, secret: true },
          { name: "公钥", previousName: null, value: pair.publicKey, secret: false },
          ...(passphrase ? [{ name: "口令", previousName: null, value: passphrase, secret: true }] : []),
        ],
        expiresAt: null, tags: [], note: "", favorite: false,
      });
      setEntry(saved);
      onSaved(saved);
    } catch (error) { toast(vaultError(error), "err"); }
    finally { setBusy(false); }
  }

  if (entry) {
    return <Modal wide title="密钥已生成" icon="ti-key" onClose={onClose}
      sub="私钥已加密保存在保管库里。把下面的公钥交给服务器，就能用这把密钥登录。"
      footer={<button className="pr sm" onClick={onClose}>完成</button>}>
      <div className="vault-keygen">
        <div className="vault-editor-label">公钥</div>
        <code className="vault-pubkey" translate="no">{publicKeyOf(entry)}</code>
        <SshKeyActions entry={entry} onChanged={setEntry} />
        <div className="vault-sub" style={{ margin: 0 }}>VPS 面板里有“SSH Key”就粘公钥；没有面板就登录服务器执行安装命令。以后在条目详情里还能再复制。</div>
      </div>
    </Modal>;
  }

  return <Modal wide title="生成 SSH 密钥" icon="ti-key" onClose={busy ? undefined : onClose}
    sub="一把密钥对应一台或一组服务器：公钥给服务器，私钥留在保管库。"
    footer={<>
      <button className="gh sm" disabled={busy} onClick={onClose}>取消</button>
      <button className="pr sm" disabled={busy || !title.trim() || (passphrase !== "" && passphrase !== again)} onClick={() => void generate()}>
        <i className={"ti " + (busy ? "ti-loader spin" : "ti-key")} /> {busy ? "正在生成…" : "生成并保存"}
      </button>
    </>}>
    <div className="vault-form vault-keygen">
      <div className="vault-editor-pair">
        <label>名称<input className="ip full" autoFocus value={title} placeholder="例如 hostinger-vps" onChange={(e) => setTitle(e.target.value)} /></label>

      </div>
      <div className="vault-editor-block">
        <span className="vault-editor-label">算法</span>
        <div className="seg">
          <button type="button" className={algorithm === "ed25519" ? "on" : ""} disabled={busy} onClick={() => setAlgorithm("ed25519")}>Ed25519</button>
          <button type="button" className={algorithm === "rsa" ? "on" : ""} disabled={busy} onClick={() => setAlgorithm("rsa")}>RSA 4096</button>
        </div>
        <span className="vault-sub" style={{ margin: 0 }}>{algorithm === "ed25519" ? "推荐：短、快、安全，现在的服务器都支持。" : "只在服务器太老、不认 Ed25519 时用；生成要等几秒。"}</span>
      </div>
      <div className="vault-editor-pair">
        <label>公钥备注<input className="ip full" value={comment} spellCheck={false} placeholder="选填，默认用名称" onChange={(e) => setComment(e.target.value)} /></label>
        <label>私钥口令<input className="ip full" type="password" autoComplete="new-password" value={passphrase} placeholder="选填，留空则不设" onChange={(e) => setPassphrase(e.target.value)} /></label>
      </div>
      {passphrase !== "" && <div className="vault-editor-pair">
        <span />
        <label>再输一次口令<input className="ip full" type="password" autoComplete="new-password" value={again} onChange={(e) => setAgain(e.target.value)} /></label>
      </div>}
      <div className="vault-sub" style={{ margin: 0 }}>口令加密私钥本身：私钥文件被拿走也用不了，但每次连接要输入口令。不设也可以，之后随时能在条目详情里设置。</div>
    </div>
  </Modal>;
}
