import { useCallback, useEffect, useState } from "react";
import { invoke } from "../invoke";
import { useI18n } from "../i18n";
import { useToast, ErrorState, Loading } from "../ui";
import { GatewayAgents } from "../features/gateway/GatewayAgents";
import { FoldCard } from "../features/gateway/Fold";
import { GatewayLog } from "../features/gateway/GatewayLog";

type LogEntry = { at: number; endpoint: string; model: string; status: number; elapsedMs: number };
type Status = { enabled: boolean; running: boolean; port: number; token: string; error: string; recent: LogEntry[]; logEnabled: boolean; logRetentionDays: number; lanAccess: boolean; addresses: string[] };

const ERRORS: Record<string, string> = { E_PORT: "端口被占用或无效（需 1024–65535），请换一个端口。" };

export default function Gateway() {
  const { tr: t } = useI18n();
  const toast = useToast();
  const [status, setStatus] = useState<Status | null>(null);
  const [port, setPort] = useState("");
  const [busy, setBusy] = useState(false);
  const [showKey, setShowKey] = useState(false);
  const [example, setExample] = useState<"curl" | "openai" | "anthropic">("curl");
  const [loadErr, setLoadErr] = useState(false);

  const load = useCallback(async () => {
    const next = await invoke<Status>("gateway_status");
    setStatus(next); setPort(String(next.port)); setLoadErr(false);
  }, []);
  useEffect(() => {
    load().catch(() => setLoadErr(true));
  }, [load]);
  useEffect(() => {
    if (!status?.running) return;
    const timer = window.setInterval(() => { void invoke<Status>("gateway_status").then(setStatus).catch(() => {}); }, 3000);
    return () => clearInterval(timer);
  }, [status?.running]);

  async function apply(enabled: boolean) {
    const p = Number(port);
    if (!Number.isInteger(p) || p < 1024 || p > 65535) { toast(t("端口需在 1024–65535 之间"), "info"); return; }
    setBusy(true);
    try {
      const next = await invoke<Status>("gateway_set", { enabled, port: p });
      setStatus(next);
      if (next.error) toast(t(ERRORS[next.error] ?? next.error), "err");
      else toast(t(enabled ? "接口服务已开启" : "接口服务已关闭"), "ok");
    } catch (e) { toast(t(ERRORS[String(e)] ?? String(e)), "err"); }
    finally { setBusy(false); }
  }
  async function setLan(enabled: boolean) {
    setBusy(true);
    try {
      setStatus(await invoke<Status>("gateway_set_lan", { enabled }));
    } catch (e) { toast(String(e), "err"); }
    finally { setBusy(false); }
  }

  async function setLogPolicy(logEnabled: boolean, retentionDays: number) {
    try {
      await invoke("gateway_set_log", { enabled: logEnabled, retentionDays });
      setStatus(await invoke<Status>("gateway_status"));
    } catch (e) { toast(String(e), "err"); }
  }

  async function regenerate() {
    setBusy(true);
    try { setStatus(await invoke<Status>("gateway_new_token")); toast(t("已生成新密钥，旧密钥立即失效"), "ok"); }
    catch (e) { toast(String(e), "err"); }
    finally { setBusy(false); }
  }
  async function copy(text: string) {
    try { await navigator.clipboard.writeText(text); toast(t("已复制"), "ok"); } catch { toast(t("复制失败，请手动选中复制"), "err"); }
  }

  if (loadErr) return <ErrorState title={t("暂时无法读取接口服务状态")} description={t("请稍后重试。")} onRetry={load} />;
  if (!status) return <Loading text={t("正在读取接口服务状态…")} />;

  const base = `http://127.0.0.1:${status.port}`;
  // The rule the user runs themselves; Stacker never changes the firewall on its own.
  const firewall = `netsh advfirewall firewall add rule name="Stacker API ${status.port}" dir=in action=allow protocol=TCP localport=${status.port} profile=private`;
  const key = showKey ? status.token : `${status.token.slice(0, 14)}••••••••••••`;
  const EXAMPLES = {
    curl: `curl ${base}/v1/chat/completions \\\n  -H "Authorization: Bearer ${status.token}" \\\n  -H "Content-Type: application/json" \\\n  -d '{"model":"claude/sonnet","reasoning_effort":"high","messages":[{"role":"user","content":"Hello"}]}'`,
    openai: `from openai import OpenAI\n\nclient = OpenAI(base_url="${base}/v1", api_key="${status.token}")\nreply = client.chat.completions.create(\n    model="codex",\n    reasoning_effort="high",  # 推理强度：省略则用页面上为该智能体设定的默认\n    messages=[{"role": "user", "content": "Hello"}],\n)\nprint(reply.choices[0].message.content)`,
    anthropic: `from anthropic import Anthropic\n\nclient = Anthropic(base_url="${base}", api_key="${status.token}")\nreply = client.messages.create(\n    model="claude/sonnet", max_tokens=1024,\n    # Anthropic 风格的请求没有推理强度字段，用页面上为该智能体设定的默认\n    messages=[{"role": "user", "content": "Hello"}],\n)\nprint(reply.content[0].text)`,
  };

  return (
    <>
      <div className={"pxhero" + (status.running ? " on" : "")}>
        <span className="pxic"><i className="ti ti-plug-connected" /></span>
        <div className="pxt">
          <div className="pxname">{t("接口服务")} <span className={"pxstat " + (status.running ? "on" : "off")}>{t(status.running ? "运行中" : "已关闭")}</span></div>
          <div className="pxsub">{t("让你自己的程序（脚本、编辑器插件、Chatbox 等支持 OpenAI / Anthropic 接口的工具）直接调用本机已登录的 Codex / Claude 对话，不用另买 API Key。只有对话能力，不能读写文件或执行命令。")}</div>
        </div>
        <div className="proxy-hero-actions">
          <label className="gw-port">{t("端口")}<input className="ip sm" value={port} disabled={busy} onChange={(e) => setPort(e.target.value.replace(/[^\d]/g, ""))} /></label>
          <label className="sw lg"><input type="checkbox" checked={status.running} disabled={busy} onChange={(e) => void apply(e.target.checked)} /><span className="tk" /></label>
        </div>
      </div>
      {status.error && <div className="callout"><i className="ti ti-alert-triangle" /><div>{t(ERRORS[status.error] ?? status.error)}</div></div>}

      <div className="pxcard">
        <div className="pxsec"><i className="ti ti-link" /> {t("接入信息")}</div>
        <div className="gw-rows">
          <div><span>OpenAI base URL</span><code>{base}/v1</code><button className="gh sm" onClick={() => void copy(`${base}/v1`)}><i className="ti ti-copy" /></button></div>
          <div><span>Anthropic base URL</span><code>{base}</code><button className="gh sm" onClick={() => void copy(base)}><i className="ti ti-copy" /></button></div>
          <div><span>API Key</span><code>{key}</code>
            <button className="gh sm" onClick={() => setShowKey(!showKey)}><i className={"ti " + (showKey ? "ti-eye-off" : "ti-eye")} /></button>
            <button className="gh sm" onClick={() => void copy(status.token)}><i className="ti ti-copy" /></button>
            <button className="gh sm" disabled={busy} onClick={() => void regenerate()}><i className="ti ti-refresh" /> {t("重新生成")}</button>
          </div>
        </div>
        <div className="gw-lan">
          <label className="sw"><input type="checkbox" checked={status.lanAccess} disabled={busy} onChange={(e) => void setLan(e.target.checked)} /><span className="tk" /></label>
          <div>
            <b>{t("允许局域网访问")}</b>
            <span>{t(status.lanAccess ? "同一网络里的设备拿到密钥就能调用，耗的是你登录的智能体额度。" : "开启后，同一网络里的设备可以用下面的地址和密钥调用本机的智能体。")}</span>
          </div>
        </div>
        {status.lanAccess && <div className="gw-rows gw-lan-rows">
          {status.addresses.map((address) => <div key={address}>
            <span>{t("局域网地址")}</span>
            <code>{`http://${address}:${status.port}`}</code>
            <button className="gh sm" onClick={() => void copy(`http://${address}:${status.port}`)}><i className="ti ti-copy" /></button>
          </div>)}
          {!status.addresses.length && <div><span>{t("局域网地址")}</span><code>{t("未读到本机的网络地址")}</code></div>}
          <p className="proxy-note">{t("列出的是本机所有网卡地址，挑与对方同一网段的那个；虚拟机走的是宿主机虚拟网卡的地址。对方如果设了系统代理，要让它绕开这个地址（curl 加 --noproxy，或把地址加进 NO_PROXY）。")}</p>
          <p className="proxy-note">{t("如果别的设备连不上，先在 Windows 防火墙里放行这个端口（以管理员身份运行）：")}</p>
          <pre className="console gw-example">{firewall}</pre>
          <button className="gh sm" onClick={() => void copy(firewall)}><i className="ti ti-copy" /> {t("复制命令")}</button>
        </div>}
        <p className="proxy-note">{t("可以发图片（claude/*、codex/*）和 PDF 文档（claude/*），文本类文档会直接并入对话；不支持工具调用。stream 请求：claude/* 逐字返回，codex/* 在生成完成后一次性返回。")}</p>
      </div>

      <GatewayAgents base={base} token={status.token} />

      <FoldCard id="examples" title={<><i className="ti ti-code" /> {t("调用示例")}</>}>
        <div className="seg" style={{ marginBottom: 10 }}>
          {([["curl", "curl"], ["openai", "OpenAI SDK"], ["anthropic", "Anthropic SDK"]] as const).map(([k, label]) => <button key={k} className={example === k ? "on" : ""} onClick={() => setExample(k)}>{label}</button>)}
        </div>
        <pre className="console gw-example">{EXAMPLES[example]}</pre>
        <button className="gh sm" onClick={() => void copy(EXAMPLES[example])}><i className="ti ti-copy" /> {t("复制")}</button>
      </FoldCard>

      <GatewayLog enabled={status.logEnabled} retentionDays={status.logRetentionDays}
        onSettings={(logEnabled, retentionDays) => void setLogPolicy(logEnabled, retentionDays)} />

      <div className="callout">
        <i className="ti ti-shield-lock" />
        <div>{status.lanAccess
          ? <><b>{t("已对局域网开放")}</b> {t("每个请求仍然需要上面的密钥，也仍然拒绝浏览器网页发起的请求。密钥就是钥匙：只给你信得过的设备，换了网络环境（例如公共 Wi-Fi）先关掉这个开关，态势不对劲时随时重新生成密钥。")}</>
          : <><b>{t("仅供本机自用")}</b> {t("服务只监听 127.0.0.1，拒绝浏览器网页发起的请求，每个请求都需要上面的密钥。调用会消耗你在对应智能体中登录账号的额度；请勿把端口或密钥提供给他人。")}</>}
        </div>
      </div>
    </>
  );
}
