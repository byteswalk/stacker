import { useCallback, useEffect, useState } from "react";
import { invoke } from "../invoke";
import { useToast, useBusy, useBusyRead, Loading, ErrorState, ConfirmModal } from "../ui";

type Mode = "hands_off" | "system" | "manual";
type ProxyStatus = {
  enabled: boolean; host: string; port: number; endpoint_available: boolean;
  no_proxy_auto: string[]; no_proxy_manual: string[];
};
type LocationRow = { id: string; value: string | null; owner: "managed" | "external" | "none" };
type Overview = { mode: Mode; host: string; port: number; windows: string | null; write_address: string | null; locations: LocationRow[] };

const MODES: { value: Mode; label: string; hint: string }[] = [
  { value: "hands_off", label: "不干预", hint: "Stacker 不会自动写入或清除任何代理设置，只在你点击下方按钮时写入。" },
  { value: "system", label: "跟随系统", hint: "启动时和点击「同步」时，把 Stacker 写入的代理更新为 Windows 系统代理地址；你自己改过的不会动。" },
  { value: "manual", label: "手动", hint: "使用下面填写的地址；同步规则与跟随系统相同。" },
];

const LOCATION_INFO: Record<string, { name: string; detail: string; icon: string }> = {
  env: { name: "终端环境变量", detail: "用户级 HTTP_PROXY / HTTPS_PROXY / ALL_PROXY / NO_PROXY，新开终端生效", icon: "ti-terminal-2" },
  git: { name: "Git", detail: "全局 http.proxy / https.proxy", icon: "ti-brand-git" },
  npm: { name: "npm / pnpm", detail: "~/.npmrc 的 proxy / https-proxy", icon: "ti-brand-npm" },
  yarn: { name: "Yarn", detail: "~/.yarnrc 的 proxy / https-proxy", icon: "ti-brand-yarn" },
  maven: { name: "Maven", detail: "~/.m2/settings.xml 中 Stacker 的代理（只改写 Stacker 生成的文件）", icon: "ti-package" },
  maven_opts: { name: "MAVEN_OPTS", detail: "用户环境变量中的 -Dhttp(s).proxy 参数", icon: "ti-variable" },
  gradle: { name: "Gradle", detail: "Stacker 的 Gradle 初始化脚本中的代理", icon: "ti-brand-gradle" },
  gradle_props: { name: "gradle.properties", detail: "~/.gradle/gradle.properties 的 systemProp.http(s).proxy", icon: "ti-file-settings" },
};

const OWNER: Record<LocationRow["owner"], { label: string; cls: string }> = {
  managed: { label: "Stacker 管理", cls: "g" },
  external: { label: "外部设置", cls: "y" },
  none: { label: "未设置", cls: "n" },
};

const ERRORS: Record<string, string> = {
  E_PROXY_ADDR: "当前没有可写入的代理地址：Windows 未开启系统代理，也没有手动地址。",
  E_EXTERNAL_FILE: "settings.xml 是你自己维护的文件，Stacker 不会改写它，请手动编辑。",
};
const errorText = (e: unknown) => ERRORS[String(e)] ?? String(e);

export default function Proxy() {
  const toast = useToast();
  const runBusy = useBusy();
  const read = useBusyRead();
  const [ov, setOv] = useState<Overview | null>(null);
  const [st, setSt] = useState<ProxyStatus | null>(null);
  const [manual, setManual] = useState<string[]>([]);
  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [busy, setBusy] = useState("");
  const [confirmClear, setConfirmClear] = useState<LocationRow | null>(null);
  const [copied, setCopied] = useState("");
  const [shell, setShell] = useState<"powershell" | "cmd" | "bash">("powershell");
  const [loadErr, setLoadErr] = useState(false);

  const load = useCallback(async () => {
    const [nextOv, nextSt] = await Promise.all([invoke<Overview>("proxy_overview"), invoke<ProxyStatus>("proxy_status")]);
    setOv(nextOv); setSt(nextSt); setManual(nextSt.no_proxy_manual);
    if (nextOv.mode === "manual") { setHost(nextOv.host); setPort(nextOv.port ? String(nextOv.port) : ""); }
    setLoadErr(false);
  }, []);
  useEffect(() => { read("正在读取代理设置", load).catch(() => setLoadErr(true)); }, [load, read]);

  async function run(key: string, task: () => Promise<unknown>, ok: string) {
    setBusy(key);
    try { await runBusy({ title: "正在应用代理设置" }, async () => { await task(); await load(); }); toast(ok, "ok"); }
    catch (e) { toast(errorText(e), "err"); }
    finally { setBusy(""); }
  }

  const setMode = (mode: Mode) => run("mode", () => invoke("settings_set_proxy_mode", { mode }), mode === "hands_off" ? "已切换为不干预：Stacker 不会再自动修改代理设置" : "已切换代理模式并同步 Stacker 管理的条目");
  const saveManual = () => {
    const p = Number(port);
    if (!host.trim() || !Number.isInteger(p) || p <= 0 || p > 65535) { toast("请输入有效的代理地址和端口", "info"); return; }
    void run("addr", () => invoke("settings_set_proxy_addr", { host: host.trim(), port: p }), "代理地址已保存，并同步了 Stacker 管理的条目");
  };
  const sync = () => run("sync", () => invoke("settings_sync_system_proxy"), "已同步 Stacker 管理的代理条目");
  const write = (row: LocationRow) => run(row.id, () => invoke("proxy_location_write", { id: row.id }), `已写入 ${LOCATION_INFO[row.id]?.name ?? row.id} 代理`);
  const clear = (row: LocationRow) => run(row.id, () => invoke("proxy_location_clear", { id: row.id }), `已清除 ${LOCATION_INFO[row.id]?.name ?? row.id} 代理`);

  async function copy(text: string, which: string) {
    try { await navigator.clipboard.writeText(text); setCopied(which); setTimeout(() => setCopied(""), 1500); toast("已复制", "ok"); }
    catch { toast("复制失败，请手动选中复制", "err"); }
  }

  if (loadErr) return <ErrorState title="暂时无法读取代理状态" description="请确认当前用户环境变量可访问，然后重试。" onRetry={load} />;
  if (!ov || !st) return <Loading text="正在读取各处代理设置…" />;

  const address = ov.write_address;
  const [h, p] = (address ?? ":").split(":");
  const httpUrl = `http://${h}:${p}`, socks = `socks5://${h}:${p}`;
  const noProxy = [...st.no_proxy_auto, ...manual].join(",");
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
  const modeInfo = MODES.find((m) => m.value === ov.mode) ?? MODES[0];

  return (
    <>
      <div className="pxhero on">
        <span className="pxic"><i className="ti ti-world-bolt" /></span>
        <div className="pxt">
          <div className="pxname">代理模式</div>
          <div className="pxsub">{modeInfo.hint}</div>
        </div>
        <div className="seg">
          {MODES.map((m) => <button key={m.value} className={ov.mode === m.value ? "on" : ""} disabled={!!busy} onClick={() => void setMode(m.value)}>{m.label}</button>)}
        </div>
      </div>

      <div className="pxcard">
        <div className="pxsec"><i className="ti ti-map-pin" /> 代理地址</div>
        <div className="proxy-address">
          <div><span>Windows 系统代理</span><b className="mono">{ov.windows ?? "未开启"}</b></div>
          {ov.mode === "manual"
            ? <div><span>手动地址</span>
              <input className="ip" value={host} placeholder="127.0.0.1" onChange={(e) => setHost(e.target.value)} style={{ width: 150 }} />
              <input className="ip sm" value={port} placeholder="端口" onChange={(e) => setPort(e.target.value.replace(/[^\d]/g, ""))} />
              <button className="pr sm" disabled={!!busy} onClick={saveManual}><i className="ti ti-device-floppy" /> 保存</button>
            </div>
            : <div><span>写入时使用</span><b className="mono">{address ?? "无（没有可用地址）"}</b></div>}
          {ov.mode !== "hands_off" && <button className="gh sm" disabled={!!busy} onClick={() => void sync()}><i className={"ti " + (busy === "sync" ? "ti-loader spin" : "ti-refresh")} /> 同步</button>}
        </div>
        {!ov.windows && ov.mode === "system" && <p className="proxy-note">Windows 当前没有开启系统代理（例如使用 TUN / VPN 或代理软件未启动）。同步会暂时撤掉 Stacker 写入的代理，但记住这些位置，系统代理恢复后再同步即可重新写入。</p>}
      </div>

      <div className="pxcard">
        <div className="pxsec"><i className="ti ti-list-details" /> 应用代理 <span className="pxhint">「外部设置」是你或其他工具配置的，Stacker 不会自动修改</span></div>
        <div className="proxy-locations">
          {ov.locations.map((row) => {
            const info = LOCATION_INFO[row.id] ?? { name: row.id, detail: "", icon: "ti-point" };
            const owner = OWNER[row.owner];
            return <div className="proxy-location" key={row.id}>
              <i className={"ti " + info.icon} />
              <div className="mt"><div className="t">{info.name} <span className={"bd " + owner.cls}>{owner.label}</span></div><div className="s dim">{info.detail}</div></div>
              <span className="mono proxy-value" title={row.value ?? ""}>{row.value ?? "—"}</span>
              <button className="gh sm" disabled={!!busy || !address} title={address ? `写入 ${address}` : "没有可用的代理地址"} onClick={() => void write(row)}>
                <i className={"ti " + (busy === row.id ? "ti-loader spin" : "ti-pencil")} /> 写入
              </button>
              <button className="gh sm" disabled={!!busy || row.owner === "none"} onClick={() => row.owner === "external" ? setConfirmClear(row) : void clear(row)}>
                <i className="ti ti-eraser" /> 清除
              </button>
            </div>;
          })}
        </div>
      </div>

      <div className="pxcard">
        <div className="pxsec"><i className="ti ti-terminal-2" /> 让已打开的终端立即生效</div>
        <div style={{ fontSize: 12, color: "var(--mut)", lineHeight: 1.65, marginBottom: 11 }}>
          环境变量<b style={{ color: "var(--tx)" }}>只对新开</b>的终端生效。已打开的窗口，按所用 shell 粘贴执行下面的片段即可立即生效。
        </div>
        <div className="seg" style={{ marginBottom: 10 }}>
          {SHELLS.map(([k, label]) => <button key={k} className={shell === k ? "on" : ""} onClick={() => setShell(k)}>{label}</button>)}
        </div>
        <div className="console" style={{ marginBottom: 11, userSelect: "text" }}>{address ? cur.on : "当前没有可用的代理地址。"}</div>
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <button className="gh sm" disabled={!address} onClick={() => copy(cur.on, "on")}><i className="ti ti-copy" /> {copied === "on" ? "已复制启用命令" : "复制启用命令"}</button>
          <button className="gh sm" onClick={() => copy(cur.off, "off")}><i className="ti ti-copy" /> {copied === "off" ? "已复制停用命令" : "复制停用命令"}</button>
        </div>
      </div>

      <div className="callout">
        <i className="ti ti-shield-half" />
        <div><b>Stacker 怎样对待你的代理设置</b> 只有 Stacker 自己写入、且之后没被改动过的条目才会被自动同步或清除。你手动配置或其他工具写入的代理一律标为「外部设置」，除非你在这里点击「清除」并确认，否则不会被修改。Stacker 不检测或配置 TUN、VPN。</div>
      </div>

      {confirmClear && <ConfirmModal title={`清除 ${LOCATION_INFO[confirmClear.id]?.name ?? confirmClear.id} 代理`} icon="ti-eraser" danger
        message={`这是你或其他工具设置的代理（${confirmClear.value}），不是 Stacker 写入的。确定要清除吗？`}
        confirmLabel="清除"
        onConfirm={() => { const row = confirmClear; setConfirmClear(null); void clear(row); }}
        onClose={() => setConfirmClear(null)} />}
    </>
  );
}
