import { useCallback, useEffect, useState } from "react";
import { invoke } from "../invoke";
import { useToast, useBusy, useBusyRead, Loading, ErrorState, ConfirmModal } from "../ui";

type ProxyStatus = {
  enabled: boolean; host: string; port: number; endpoint_available: boolean;
  no_proxy_auto: string[]; no_proxy_manual: string[];
};
type LocationRow = { id: string; value: string | null; owner: "managed" | "external" | "none" };
type Overview = { host: string; port: number; windows: string | null; write_address: string | null; locations: LocationRow[] };

type SystemState = "on" | "off" | "stale" | "unknown";
type SystemProxy = { state: SystemState; server: string; recorded: string; bypass: string };
type ServiceProxy = { server: string; bypass: string; known: boolean };
type SyncReport = { system: SystemProxy; service: ServiceProxy; rows: unknown[] };

/** Windows' own setting, which every row on this page follows. */
const SYSTEM_STATE: Record<SystemState, { label: string; on: boolean; hint: string }> = {
  on: { label: "系统代理已开启", on: true, hint: "下面各处写入时都用这个地址；系统代理关掉后一并撤销。" },
  off: { label: "系统代理未开启", on: false, hint: "没有可写入的地址。下面仍写着代理的位置应该撤销，否则请求会发去一个没人监听的端口。" },
  stale: { label: "系统代理设置残留", on: false, hint: "注册表里还写着地址，但连接记录说直连；新启动的程序不会走它。" },
  unknown: { label: "读不到系统代理", on: false, hint: "读不到 Windows 的代理设置。" },
};

const LOCATION_INFO: Record<string, { name: string; detail: string; icon: string }> = {
  env: { name: "终端环境变量", detail: "用户级 HTTP_PROXY / HTTPS_PROXY / ALL_PROXY，新开终端生效", icon: "ti-terminal-2" },
  winhttp: { name: "服务代理 WinHTTP", detail: "系统服务使用，修改需要管理员权限", icon: "ti-settings-cog" },
  git: { name: "Git", detail: "全局 http.proxy / https.proxy", icon: "ti-brand-git" },
  npm: { name: "npm / pnpm", detail: "~/.npmrc 的 proxy / https-proxy", icon: "ti-brand-npm" },
  yarn: { name: "Yarn", detail: "~/.yarnrc 的 proxy / https-proxy", icon: "ti-brand-yarn" },
  maven: { name: "Maven", detail: "~/.m2/settings.xml 中 Stacker 的代理", icon: "ti-package" },
  maven_opts: { name: "MAVEN_OPTS", detail: "用户环境变量中的 -Dhttp(s).proxy 参数", icon: "ti-variable" },
  gradle: { name: "Gradle", detail: "Stacker 的 Gradle 初始化脚本中的代理", icon: "ti-brand-gradle" },
  gradle_props: { name: "gradle.properties", detail: "~/.gradle/gradle.properties 的 systemProp.http(s).proxy", icon: "ti-file-settings" },
};

const ERRORS: Record<string, string> = {
  E_PROXY_ADDR: "Windows 没有开启系统代理，没有可写入的地址。",
  E_EXTERNAL_FILE: "settings.xml 是你自己维护的文件，Stacker 不会改写它，请手动编辑。",
};
const errorText = (e: unknown) => ERRORS[String(e)] ?? String(e);

/** 一行的状态只有三种：没设置、跟系统一致、该撤销或该改。 */
function rowState(value: string | null, system: SystemProxy) {
  const current = (value ?? "").trim();
  if (!current) return { label: "尚未设置", cls: "n" as const, stale: false };
  const on = SYSTEM_STATE[system.state].on;
  if (!on) return { label: "待撤销", cls: "r" as const, stale: true };
  const endpoint = current.replace(/^\w+:\/\//, "").replace(/\/$/, "");
  return endpoint === system.server
    ? { label: "已设置", cls: "g" as const, stale: false }
    : { label: "与系统不同", cls: "y" as const, stale: false };
}

export default function Proxy() {
  const toast = useToast();
  const runBusy = useBusy();
  const read = useBusyRead();
  const [ov, setOv] = useState<Overview | null>(null);
  const [st, setSt] = useState<ProxyStatus | null>(null);
  const [manual, setManual] = useState<string[]>([]);
  const [busy, setBusy] = useState("");
  const [confirmClear, setConfirmClear] = useState<LocationRow | null>(null);
  const [confirmRelease, setConfirmRelease] = useState(false);
  const [copied, setCopied] = useState("");
  const [shell, setShell] = useState<"powershell" | "cmd" | "bash">("powershell");
  const [loadErr, setLoadErr] = useState(false);
  const [report, setReport] = useState<SyncReport | null>(null);

  const load = useCallback(async () => {
    const [nextOv, nextSt, nextReport] = await Promise.all([
      invoke<Overview>("proxy_overview"),
      invoke<ProxyStatus>("proxy_status"),
      invoke<SyncReport>("proxy_sync_report"),
    ]);
    setOv(nextOv); setSt(nextSt); setReport(nextReport); setManual(nextSt.no_proxy_manual);
    setLoadErr(false);
  }, []);
  useEffect(() => { read("正在读取代理设置", load).catch(() => setLoadErr(true)); }, [load, read]);

  async function run(key: string, task: () => Promise<unknown>, ok: string) {
    setBusy(key);
    try { await runBusy({ title: "正在应用代理设置" }, async () => { await task(); await load(); }); toast(ok, "ok"); }
    catch (e) { toast(errorText(e), "err"); await load().catch(() => undefined); }
    finally { setBusy(""); }
  }

  const setService = (address: string | null) => run(
    "winhttp",
    () => invoke("proxy_service_set", { address }),
    address ? "服务代理已跟随系统" : "服务代理已清除",
  );
  const write = (row: LocationRow) => run(row.id, () => invoke("proxy_location_write", { id: row.id }), `已写入 ${LOCATION_INFO[row.id]?.name ?? row.id} 代理`);
  const clear = (row: LocationRow) => run(row.id, () => invoke("proxy_location_clear", { id: row.id }), `已清除 ${LOCATION_INFO[row.id]?.name ?? row.id} 代理`);
  const followAll = () => run("all", () => invoke("proxy_follow_system", { release: false }), "各处代理已跟随系统");
  const releaseAll = () => run("all", () => invoke("proxy_follow_system", { release: true }), "各处代理已撤销");

  async function copy(text: string, which: string) {
    try { await navigator.clipboard.writeText(text); setCopied(which); setTimeout(() => setCopied(""), 1500); toast("已复制", "ok"); }
    catch { toast("复制失败，请手动选中复制", "err"); }
  }

  if (loadErr) return <ErrorState title="暂时无法读取代理状态" description="请确认当前用户环境变量可访问，然后重试。" onRetry={load} />;
  if (!ov || !st || !report) return <Loading text="正在读取各处代理设置…" />;

  const system = report.system;
  const info = SYSTEM_STATE[system.state];
  const address = info.on ? system.server : null;
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

  // The service proxy is Windows' own setting, but it sits in the same list: one place to
  // see what is set where.
  const serviceRow: LocationRow = {
    id: "winhttp",
    value: report.service.known ? (report.service.server || null) : null,
    owner: report.service.server ? "external" : "none",
  };
  const rows = [ov.locations[0], serviceRow, ...ov.locations.slice(1)].filter(Boolean);
  const anySet = rows.some((row) => !!(row.value ?? "").trim());

  return (
    <>
      <div className={"pxhero" + (info.on ? " on" : "")}>
        <span className="pxic"><i className="ti ti-world-bolt" /></span>
        <div className="pxt">
          <div className="pxname">{info.label}{info.on && <b className="mono">{system.server}</b>}</div>
          <div className="pxsub">{info.hint}</div>
        </div>
        {info.on
          ? <button className="pr sm" disabled={!!busy} onClick={() => void followAll()}>
            <i className={"ti " + (busy === "all" ? "ti-loader spin" : "ti-arrow-down-to-arc")} /> 全部跟随系统
          </button>
          : <button className="gh sm" disabled={!!busy || !anySet} onClick={() => setConfirmRelease(true)}>
            <i className={"ti " + (busy === "all" ? "ti-loader spin" : "ti-eraser")} /> 全部撤销
          </button>}
      </div>

      <div className="pxcard">
        <div className="pxsec"><i className="ti ti-list-details" /> 应用代理 <span className="pxhint">写入用的就是上面的系统代理地址</span></div>
        <div className="proxy-locations">
          {rows.map((row) => {
            const meta = LOCATION_INFO[row.id] ?? { name: row.id, detail: "", icon: "ti-point" };
            const state = rowState(row.value, system);
            const service = row.id === "winhttp";
            const writable = info.on && (service ? report.service.known : true);
            return <div className="proxy-location" key={row.id}>
              <i className={"ti " + meta.icon} />
              <div className="mt">
                <div className="t"><span className="nm">{meta.name}</span><span className={"bd " + state.cls}>{state.label}</span></div>
                <div className="proxy-sub">
                  <span className="s dim" title={meta.detail}>{meta.detail}</span>
                  <span className="mono proxy-value" title={row.value ?? ""}>{row.value ?? "—"}</span>
                </div>
              </div>
              <button className="gh sm" disabled={!!busy || !writable || state.label === "已设置"}
                title={info.on ? `写入 ${system.server}` : "系统代理未开启，没有可写入的地址"}
                onClick={() => service ? void setService(system.server) : void write(row)}>
                <i className={"ti " + (busy === row.id ? "ti-loader spin" : "ti-pencil")} /> 写入
              </button>
              <button className="gh sm" disabled={!!busy || !(row.value ?? "").trim()}
                onClick={() => service ? void setService(null) : (row.owner === "external" ? setConfirmClear(row) : void clear(row))}>
                <i className="ti ti-eraser" /> 清除
              </button>
            </div>;
          })}
        </div>
      </div>

      <div className="pxcard">
        <div className="pxsec"><i className="ti ti-terminal-2" /> 让已打开的终端立即生效 <span className="pxhint">环境变量只对新开的终端生效</span></div>
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
        <div><b>Stacker 怎样对待你的代理设置</b> 这一页只读 Windows 的系统代理，从不改它。上面各处的写入和清除都由你点击触发，Stacker 不会在后台动它们。TUN、VPN 由代理软件负责，Stacker 不检测也不配置。</div>
      </div>

      {confirmClear && <ConfirmModal title={`清除 ${LOCATION_INFO[confirmClear.id]?.name ?? confirmClear.id} 代理`} icon="ti-eraser" danger
        message={`这是你或其他工具设置的代理（${confirmClear.value}），不是 Stacker 写入的。确定要清除吗？`}
        confirmLabel="清除"
        onConfirm={() => { const row = confirmClear; setConfirmClear(null); void clear(row); }}
        onClose={() => setConfirmClear(null)} />}

      {confirmRelease && <ConfirmModal title="撤销所有位置的代理" icon="ti-eraser" danger
        message="系统代理已经关闭，下面仍写着代理的位置都会被清除，包括不是 Stacker 写入的。确定吗？"
        confirmLabel="全部撤销"
        onConfirm={() => { setConfirmRelease(false); void releaseAll(); }}
        onClose={() => setConfirmRelease(false)} />}
    </>
  );
}
