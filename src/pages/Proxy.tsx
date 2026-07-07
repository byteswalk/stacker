import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useToast, Loading } from "../ui";

type ProxyStatus = {
  enabled: boolean; http: string; host: string; port: number;
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
  useEffect(() => {
    (async () => {
      const s = await invoke<ProxyStatus>("proxy_status");
      setSt(s);
      setManual(s.no_proxy_manual);
    })().catch(() => setLoadErr(true));
  }, []);

  async function toggle() {
    setBusy(true);
    try {
      if (st?.enabled) await invoke("proxy_disable", { alsoJvm: false });
      else await invoke("proxy_enable", { host: st?.host || "127.0.0.1", port: st?.port || 7890, alsoJvm: false, manual });
      await refresh();
      toast(st?.enabled ? "已关闭终端代理" : "已开启终端代理（新终端生效）", "ok");
    } catch (e) { toast("操作失败：" + e, "err"); } finally { setBusy(false); }
  }
  function addEntry() {
    const e = entry.trim();
    if (e && !manual.includes(e) && !st?.no_proxy_auto.includes(e)) setManual([...manual, e]);
    setEntry("");
  }
  async function copy(text: string, which: string) {
    try { await navigator.clipboard.writeText(text); setCopied(which); setTimeout(() => setCopied(""), 1500); toast("已复制", "ok"); }
    catch { toast("复制失败，请手动选中复制", "err"); }
  }

  if (loadErr) return <div className="stub"><div className="si"><i className="ti ti-plug-x" /></div><h2>读取代理状态失败</h2><p>请在 Tauri 应用内运行（浏览器预览没有后端）。</p></div>;
  if (!st) return <Loading text="正在读取代理状态…" />;

  const host = st.host || "127.0.0.1";
  const pn = st.port || st.detected_port || 7890;
  const httpUrl = `http://${host}:${pn}`, socks = `socks5://${host}:${pn}`;
  const noProxy = [...st.no_proxy_auto, ...manual].join(",");
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
      <div className={"pxhero" + (st.enabled ? " on" : "")}>
        <span className="pxic"><i className="ti ti-world-bolt" /></span>
        <div className="pxt">
          <div className="pxname">终端代理 <span className={"pxstat " + (st.enabled ? "on" : "off")}>{st.enabled ? "已开启" : "未开启"}</span></div>
          <div className="pxsub">
            <i className="ti ti-settings" style={{ color: "#6ab0f5" }} />
            {st.enabled ? "已设 HTTP_PROXY / HTTPS_PROXY / ALL_PROXY，仅对新开终端生效" : "开启后设置终端代理环境变量，对新开终端生效"}
          </div>
        </div>
        <label className="sw lg"><input type="checkbox" checked={st.enabled} disabled={busy} onChange={toggle} /><span className="tk" /></label>
      </div>

      <div className="pxcard">
        <div className="pxsec"><i className="ti ti-route-off" /> 直连白名单 NO_PROXY <span className="pxhint">名单内主机直连，不经过终端代理</span></div>
        <div className="chips" style={{ marginBottom: 11 }}>
          {st.no_proxy_auto.map((h) => <span className="chip auto" key={h}>{h} <span className="tag">自动</span></span>)}
          {manual.map((h) => <span className="chip" key={h}>{h} <i className="ti ti-x x" onClick={() => setManual(manual.filter((x) => x !== h))} /></span>)}
        </div>
        <div className="npadd">
          <input className="ip wide" placeholder="追加域名 / 主机，如 gitlab.mycorp.com" value={entry}
            onChange={(e) => setEntry(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") addEntry(); }} />
          <button className="gh sm" onClick={addEntry}>添加</button>
        </div>
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
        <div className="console" style={{ marginBottom: 11, userSelect: "text" }}>{cur.on}</div>
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <button className="gh sm" onClick={() => copy(cur.on, "on")}><i className="ti ti-copy" /> {copied === "on" ? "已复制启用命令" : "复制启用命令"}</button>
          <button className="gh sm" onClick={() => copy(cur.off, "off")}><i className="ti ti-copy" /> {copied === "off" ? "已复制停用命令" : "复制停用命令"}</button>
        </div>
      </div>

      <div className="callout">
        <i className="ti ti-shield-half" />
        <div><b>为什么用环境变量而非 TUN？</b> 只让<b>终端</b>走代理、精确可控、不怕系统别处偷跑流量；不在意就去代理软件开 TUN（全系统透明代理，无需环境变量）。</div>
      </div>
    </>
  );
}
