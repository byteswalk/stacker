import { useCallback, useEffect, useState } from "react";
import { invoke } from "../invoke";
import { useToast, Loading, ErrorState } from "../ui";

type ProxyStatus = {
  enabled: boolean; http: string; host: string; port: number;
  mode: "system" | "manual" | "off"; endpoint_available: boolean; route_only: boolean;
  detected_port: number | null; no_proxy_auto: string[]; no_proxy_manual: string[];
};

export default function Proxy() {
  const toast = useToast();
  const [st, setSt] = useState<ProxyStatus | null>(null);
  const [manual, setManual] = useState<string[]>([]);
  const [entry, setEntry] = useState("");
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState("");
  const [shell, setShell] = useState<"powershell" | "cmd" | "bash">("powershell");
  const [loadErr, setLoadErr] = useState(false);

  async function refresh() { setSt(await invoke<ProxyStatus>("proxy_status")); }
  async function syncSystemProxy() {
    setBusy(true);
    try {
      await invoke("settings_sync_system_proxy");
      const next = await invoke<ProxyStatus>("proxy_status");
      setSt(next);
      setManual(next.no_proxy_manual);
      toast(next.endpoint_available ? `已同步系统代理 ${next.host}:${next.port}` : "Windows 未提供显式代理地址，已清除旧代理配置", "ok");
    } catch (e) {
      toast("同步系统代理失败：" + e, "err");
    } finally {
      setBusy(false);
    }
  }
  const loadProxy = useCallback(async () => {
    const s = await invoke<ProxyStatus>("proxy_status");
    setSt(s);
    setManual(s.no_proxy_manual);
    setLoadErr(false);
  }, []);
  useEffect(() => { loadProxy().catch(() => setLoadErr(true)); }, [loadProxy]);

  async function toggle() {
    setBusy(true);
    try {
      if (st?.enabled) {
        if (manualDirty) await invoke("settings_set_proxy_manual", { manual });
        await invoke("proxy_disable", { alsoJvm: false });
      }
      else {
        if (!st?.endpoint_available) {
          toast("当前没有显式代理地址，无需开启终端代理；网络请求将使用 Windows 当前路由。", "info");
          return;
        }
        await invoke("proxy_enable", { host: st.host, port: st.port, alsoJvm: false, manual });
      }
      const next = await invoke<ProxyStatus>("proxy_status");
      setSt(next);
      setManual(next.no_proxy_manual);
      toast(st?.enabled ? "已关闭终端代理" : "已开启终端代理（新终端生效）", "ok");
    } catch (e) { toast("操作失败：" + e, "err"); } finally { setBusy(false); }
  }
  function addEntry() {
    const e = entry.trim();
    if (e && !manual.includes(e) && !st?.no_proxy_auto.includes(e)) setManual([...manual, e]);
    setEntry("");
  }
  async function saveManual() {
    setBusy(true);
    try {
      const saved = await invoke<string[]>("settings_set_proxy_manual", { manual });
      setManual(saved);
      await refresh();
      toast("直连白名单已保存", "ok");
    } catch (e) {
      toast("保存直连白名单失败：" + e, "err");
    } finally {
      setBusy(false);
    }
  }
  async function copy(text: string, which: string) {
    try { await navigator.clipboard.writeText(text); setCopied(which); setTimeout(() => setCopied(""), 1500); toast("已复制", "ok"); }
    catch { toast("复制失败，请手动选中复制", "err"); }
  }

  if (loadErr) return <ErrorState title="暂时无法读取终端代理状态" description="请确认当前用户环境变量可访问，然后重试。" onRetry={loadProxy} />;

  const proxyLoading = !st;
  const proxyState: ProxyStatus = st ?? {
    enabled: false,
    http: "",
    host: "",
    port: 0,
    mode: "system",
    endpoint_available: false,
    route_only: true,
    detected_port: null,
    no_proxy_auto: [],
    no_proxy_manual: [],
  };
  const host = proxyState.host;
  const manualDirty = manual.join("\u0000") !== proxyState.no_proxy_manual.join("\u0000");
  const pn = proxyState.port;
  const httpUrl = `http://${host}:${pn}`, socks = `socks5://${host}:${pn}`;
  const noProxy = [...proxyState.no_proxy_auto, ...manual].join(",");
  // 三种 shell 的「立即生效 / 撤销」片段（随地址、白名单实时变）
  const SNIP: Record<typeof shell, { on: string; off: string }> = {
    powershell: {
      on: `$env:HTTP_PROXY="${httpUrl}"; $env:HTTPS_PROXY="${httpUrl}"; $env:ALL_PROXY="${socks}"; $env:NO_PROXY="${noProxy}"`,
      off: `Remove-Item Env:HTTP_PROXY,Env:HTTPS_PROXY,Env:ALL_PROXY,Env:NO_PROXY -ErrorAction SilentlyContinue`,
    },
    cmd: {
      on: `set HTTP_PROXY=${httpUrl} && set HTTPS_PROXY=${httpUrl} && set ALL_PROXY=${socks} && set NO_PROXY=${noProxy}`,
      off: `set HTTP_PROXY= && set HTTPS_PROXY= && set ALL_PROXY= && set NO_PROXY=`,
    },
    bash: {
      on: `export HTTP_PROXY="${httpUrl}" HTTPS_PROXY="${httpUrl}" ALL_PROXY="${socks}" NO_PROXY="${noProxy}"`,
      off: `unset HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY`,
    },
  };
  const SHELLS: [typeof shell, string][] = [["powershell", "PowerShell"], ["cmd", "cmd"], ["bash", "Git Bash"]];
  const cur = SNIP[shell];

  return (
    <>
      <div className={"pxhero" + (proxyState.enabled ? " on" : "")}>
        <span className="pxic"><i className="ti ti-world-bolt" /></span>
        <div className="pxt">
          <div className="pxname">终端代理 <span className={"pxstat " + (proxyState.enabled ? "on" : "off")}>{proxyLoading ? "检测中" : proxyState.enabled ? "已开启" : "未开启"}</span></div>
          <div className="pxsub">
            <i className="ti ti-settings" style={{ color: "#6ab0f5" }} />
            {proxyLoading ? "正在读取当前用户的终端代理环境变量…" : proxyState.enabled ? "已设 HTTP_PROXY / HTTPS_PROXY / ALL_PROXY，仅对新开终端生效" : "开启后设置终端代理环境变量，对新开终端生效"}
          </div>
        </div>
        <div className="proxy-hero-actions">
          {proxyState.mode === "system" && <button className="gh sm" disabled={busy || proxyLoading} onClick={syncSystemProxy}><i className="ti ti-refresh" /> 同步系统</button>}
          <label className="sw lg" title={!proxyState.endpoint_available ? "未检测到显式代理地址，无需开启终端代理" : undefined}><input type="checkbox" checked={proxyState.enabled} disabled={busy || proxyLoading || !proxyState.endpoint_available} onChange={toggle} /><span className="tk" /></label>
        </div>
      </div>

      {!proxyLoading && proxyState.route_only && <div className="callout proxy-route-callout">
        <i className="ti ti-route" />
        <div><b>未配置显式系统代理</b> 终端和开发工具无需写入 HTTP_PROXY，网络请求将使用 Windows 当前路由。若之前使用过旧代理端口，点击「同步系统」可清除旧配置。</div>
      </div>}

      <div className={"pxcard" + (!proxyState.endpoint_available ? " disabled-section" : "")}>
        <div className="pxsec"><i className="ti ti-route-off" /> 直连白名单 NO_PROXY <span className="pxhint">名单内主机直连，不经过终端代理</span></div>
        {proxyLoading && <Loading text="正在读取直连白名单…" />}
        {!proxyLoading && (
        <>
        <div className="chips" style={{ marginBottom: 11 }}>
          {proxyState.no_proxy_auto.map((h) => <span className="chip auto" key={h}>{h} <span className="tag">自动</span></span>)}
          {manual.map((h) => <span className="chip" key={h}>{h} <i className="ti ti-x x" onClick={() => setManual(manual.filter((x) => x !== h))} /></span>)}
        </div>
        <div className="npadd">
          <input className="ip wide" placeholder="追加域名 / 主机，如 gitlab.mycorp.com" value={entry}
            onChange={(e) => setEntry(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") addEntry(); }} />
          <button className="gh sm" onClick={addEntry}>添加</button>
          <button className={manualDirty ? "pr sm" : "gh sm"} disabled={busy || !manualDirty} onClick={saveManual}><i className="ti ti-device-floppy" /> 保存白名单</button>
        </div>
        </>
        )}
      </div>

      <div className="pxcard">
        <div className="pxsec"><i className="ti ti-terminal-2" /> 让已打开的终端立即生效</div>
        <div style={{ fontSize: 12, color: "var(--mut)", lineHeight: 1.65, marginBottom: 11 }}>
          主开关改的是环境变量，<b style={{ color: "var(--tx)" }}>仅对新开</b>的终端生效。已打开的窗口，按下面所用 shell 粘贴执行对应片段即可立即生效，无需重开（片段随上面的地址 / 白名单实时更新）。
        </div>
        <div className="seg" style={{ marginBottom: 10 }}>
          {SHELLS.map(([k, label]) => (
            <button key={k} className={shell === k ? "on" : ""} onClick={() => setShell(k)}>{label}</button>
          ))}
        </div>
        <div className="console" style={{ marginBottom: 11, userSelect: "text" }}>{proxyState.endpoint_available ? cur.on : "当前没有显式代理地址，无需执行终端代理启用命令。"}</div>
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <button className="gh sm" disabled={!proxyState.endpoint_available} onClick={() => copy(cur.on, "on")}><i className="ti ti-copy" /> {copied === "on" ? "已复制启用命令" : "复制启用命令"}</button>
          <button className="gh sm" onClick={() => copy(cur.off, "off")}><i className="ti ti-copy" /> {copied === "off" ? "已复制停用命令" : "复制停用命令"}</button>
        </div>
      </div>

      <div className="callout">
        <i className="ti ti-shield-half" />
        <div><b>终端代理的作用范围</b> Stacker 只读取 Windows 明确提供的代理地址，并管理当前用户的代理环境变量，不检测或配置 TUN、VPN。未提供显式代理地址时，网络请求直接使用 Windows 当前路由。</div>
      </div>
    </>
  );
}
