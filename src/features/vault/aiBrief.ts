import type { CredentialTarget, EntryView, EnvHolder, SshLocal } from "./api";
import { KIND_LABELS } from "./labels";
import { firstServer, parseTarget, publicKeyOf, serversOf, SERVERS_FIELD } from "./SshKeys";

type Tr = (text: string) => string;

/**
 * PowerShell that reads one generic credential into `$secret`. Credential Manager has no
 * cmdlet for this, so it calls CredReadW itself; the offsets are those of CREDENTIALW on
 * 64-bit Windows (blob size at 32, blob pointer at 40). The name is quoted for PowerShell.
 */
export function credentialCommand(target: string): string {
  const name = target.replace(/'/g, "''");
  return `$a=Add-Type -Name Cred -Namespace StackerWin -PassThru -MemberDefinition '[DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)]public static extern bool CredReadW(string target,int type,int flags,out IntPtr cred);[DllImport("advapi32.dll")]public static extern void CredFree(IntPtr cred);'; $p=[IntPtr]::Zero; if($a::CredReadW('${name}',1,0,[ref]$p)){ $m=[Runtime.InteropServices.Marshal]; $secret=$m::PtrToStringUni($m::ReadIntPtr($p,40),$m::ReadInt32($p,32)/2); $a::CredFree($p) }`;
}

/** `user@host` and its port, for a command line; empty when the entry does not say. */
function sshTarget(entry: EntryView): { target: string; port: string } {
  const { user, host, port } = parseTarget(firstServer(entry));
  return { target: host ? `${user || "root"}@${host}` : "", port };
}

/**
 * What an AI agent needs to act with an entry, as text to paste into it. Secrets never go in:
 * an SSH key is named by the file ssh already uses, anything else by the name it has in
 * Windows Credential Manager, with the command that reads it.
 */
export function aiBrief(entry: EntryView, local: SshLocal | null, tr: Tr, holders: EnvHolder[] = [], credentials: CredentialTarget[] = []): string {
  const lines = [`${tr("名称")}: ${entry.title}`, `${tr("类型")}: ${tr(KIND_LABELS[entry.kind])}`];
  if (entry.platform) lines.push(`${tr("平台")}: ${entry.platform}`);

  if (entry.kind === "ssh_key") {
    const { target, port } = sshTarget(entry);
    const servers = serversOf(entry);
    if (target) lines.push(`${tr("主机")}: ${target}${port && port !== "22" ? `:${port}` : ""}`);
    if (local?.alias) lines.push(`${tr("连接命令")}: ssh ${local.alias}`);
    else if (local?.path) lines.push(`${tr("连接命令")}: ssh -i "${local.path}"${port && port !== "22" ? ` -p ${port}` : ""} ${target || "<user>@<host>"}`);
    else lines.push(`${tr("连接命令")}: ${tr("私钥还没有放到本机 ~/.ssh，先在 Stacker 的密钥保管里点「放到本机 ~/.ssh」。")}`);
    if (servers.length) lines.push(`${tr("已装服务器")}: ${servers.join(", ")}`);
    if (entry.ssh?.encrypted) lines.push(tr("私钥有口令：连接时由我来输入。"));
    const publicKey = publicKeyOf(entry);
    if (publicKey) lines.push(`${tr("公钥")}: ${publicKey}`);
    if (entry.ssh?.fingerprint) lines.push(`${tr("指纹")}: ${entry.ssh.fingerprint}`);
  } else {
    for (const field of entry.fields) {
      if (!field.filled) continue;
      if (!field.secret) { lines.push(`${tr(field.name)}: ${field.value ?? ""}`); continue; }
      // The value stays on this computer: the agent is told where to read it from.
      const credential = credentials.find((item) => item.field === field.name);
      const holder = holders.find((item) => item.field === field.name);
      if (credential) lines.push(`${tr(field.name)}: ${tr("在 Windows 凭据管理器里（普通凭据），凭据名")} ${credential.target}`);
      if (holder) lines.push(`${tr(field.name)}: ${tr(holder.scope === "user" ? "在本机的用户环境变量里" : "在本机的系统环境变量里")} ${holder.name}（PowerShell: $env:${holder.name} · cmd: %${holder.name}% · bash: $${holder.name}）`);
      if (!credential && !holder) lines.push(`${tr(field.name)}: ${tr(entry.windows
        ? "（保密，未包含：这个值太长，放不进 Windows 凭据管理器）"
        : "（保密，未包含：这一条只在 Stacker 保管库里，没有放进 Windows 凭据管理器）")}`);
    }
  }
  if (entry.kind === "ssh_key") {
    for (const field of entry.fields) {
      if (field.secret || !field.filled || ["公钥", "用途/主机", SERVERS_FIELD].includes(field.name)) continue;
      lines.push(`${tr(field.name)}: ${field.value ?? ""}`);
    }
  }
  if (entry.expiresAt) lines.push(`${tr("到期")}: ${entry.expiresAt}`);
  if (entry.tags.length) lines.push(`${tr("标签")}: ${entry.tags.join(", ")}`);
  if (entry.note.trim()) lines.push(`${tr("备注")}: ${entry.note.trim()}`);
  if (entry.kind !== "ssh_key" && credentials.length) {
    lines.push(`${tr("取值命令（PowerShell，把值读进变量 $secret；换别的凭据只改命令里的凭据名）")}:`);
    lines.push(credentialCommand(credentials[0].target));
  }
  lines.push(tr(entry.kind === "ssh_key"
    ? "说明：这是我本机的 SSH 密钥信息，私钥和口令不在这里。请用上面的连接命令操作服务器，不要要求我贴出私钥。"
    : credentials.length
      ? "说明：这是我保管的一条凭据的信息，值不在这里。需要时用上面的命令从 Windows 凭据管理器读进变量直接使用，不要把值打印出来、写进文件或提交到仓库，也不要要求我贴出来。"
      : holders.length
        ? "说明：这是我保管的一条凭据的信息，值不在这里。请在命令里直接引用上面的环境变量，不要把它的值打印或写进文件。"
        : "说明：这是我保管的一条凭据的信息，保密的值不在这里，也不要要求我贴出来。"));
  return lines.join("\n");
}
