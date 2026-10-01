import { useState } from "react";
import { useI18n } from "../../i18n";
import { AiAskModal, AiButton, askAi } from "../ai/AiAsk";

export type LanAddress = { host: string; adapter: string; kind: "lan" | "hostname" | "tailscale" | "vm" | "vpn" | "other" };

/** What each kind of adapter is, and who can reach the service through it. */
const KINDS: Record<LanAddress["kind"], { label: string; note: string; tone: string }> = {
  lan: { label: "局域网", note: "同一路由器下的设备用这个", tone: "ok" },
  hostname: { label: "主机名", note: "IP 变了也能用；对方要能解析 .local 名称", tone: "info" },
  tailscale: { label: "Tailscale", note: "对方也在同一个 Tailscale 网络里才能用", tone: "pro" },
  vm: { label: "虚拟机", note: "只给本机的虚拟机用", tone: "mut" },
  vpn: { label: "VPN", note: "VPN 的虚拟网卡，一般用不上", tone: "mut" },
  other: { label: "其他", note: "", tone: "mut" },
};

/** Kinds a caller rarely needs; they stay folded until asked for. */
const FOLDED: LanAddress["kind"][] = ["vm", "vpn"];

/** The addresses the service answers on, each labelled with the adapter it belongs to. */
export function LanAddresses({ addresses, port, onCopy }: {
  addresses: LanAddress[];
  port: number;
  onCopy: (text: string) => void;
}) {
  const { tr } = useI18n();
  const [open, setOpen] = useState(false);
  const [asking, setAsking] = useState(false);
  const url = (host: string) => `http://${host}:${port}`;
  const shown = addresses.filter((a) => !FOLDED.includes(a.kind));
  const folded = addresses.filter((a) => FOLDED.includes(a.kind));
  const summary = FOLDED
    .map((kind) => [kind, folded.filter((a) => a.kind === kind).length] as const)
    .filter(([, count]) => count > 0)
    .map(([kind, count]) => `${tr(KINDS[kind].label)}${count > 1 ? ` ×${count}` : ""}`)
    .join("、");

  const row = (a: LanAddress) => <div key={a.host} className="gw-addr">
    <span className={"gw-addr-kind " + KINDS[a.kind].tone} title={a.adapter || undefined}>{tr(KINDS[a.kind].label)}</span>
    <code>{url(a.host)}</code>
    <span className="gw-addr-note" title={a.adapter || undefined}>{tr(KINDS[a.kind].note) || a.adapter}</span>
    <button className="gh sm" aria-label={tr("复制")} onClick={() => onCopy(url(a.host))}><i className="ti ti-copy" /></button>
  </div>;

  if (!addresses.length) {
    return <div className="gw-addrs"><div className="gw-addr"><span className="gw-addr-note">{tr("未读到本机的网络地址")}</span></div></div>;
  }
  return <div className="gw-addrs">
    {shown.map(row)}
    {open && folded.map(row)}
    <div className="gw-addr gw-addr-foot">
      {folded.length > 0
        ? <button className="gh sm gw-addr-more" onClick={() => setOpen(!open)}>
          <i className={"ti " + (open ? "ti-chevron-down" : "ti-chevron-right")} />
          {open ? tr("收起") : `${tr("另外 {count} 个：").replace("{count}", String(folded.length))}${summary}`}
        </button>
        : <span />}
      <AiButton label={tr("该用哪个？")} title={tr("把这些地址和网卡交给 AI，说清各自能给谁用")} onClick={() => setAsking(true)} />
    </div>
    {asking && <AiAskModal title={tr("该用哪个地址")} note={tr("发给 AI 的是：这些地址、对应的网卡名和端口。")}
      run={() => askAi("lan_address", { port, addresses: addresses.map((a) => ({ address: url(a.host), adapter: a.adapter || tr("主机名"), kind: a.kind })) })}
      onClose={() => setAsking(false)} />}
  </div>;
}
