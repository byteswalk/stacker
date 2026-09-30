import { useCallback, useEffect, useState } from "react";
import { invoke } from "../invoke";
import { useToast, useBusy, useBusyRead, Loading, ErrorState, ConfirmModal, Modal } from "../ui";
import { Select } from "../Select";
import { AiAskModal, AiButton, askAi } from "../features/ai/AiAsk";

type ProxyStatus = {
  enabled: boolean; host: string; port: number; endpoint_available: boolean;
  no_proxy_auto: string[]; no_proxy_manual: string[];
};
type Entry = { section: string; key: string; value: string };
type Detect = { type: "path_exists"; path: string } | { type: "command_on_path"; command: string };
type Writer = { format: "key_value" | "ini" | "json"; path: string; entries: Entry[] };
/** A program whose proxy lives in a file of its own, described rather than coded. */
type Target = { id: string; name: string; detail: string; icon: string; builtin: boolean; detect: Detect; writer: Writer };
type LocationRow = {
  id: string; value: string | null; owner: "managed" | "external" | "none";
  target?: Target; installed?: boolean;
};

const FORMATS: { value: Writer["format"]; label: string }[] = [
  { value: "key_value", label: "键=值（.env / .npmrc / .curlrc）" },
  { value: "ini", label: "分节 ini（pip.ini / Cargo config.toml）" },
  { value: "json", label: "JSON（settings.json，键写成 /a/b）" },
];

const BLANK_TARGET: Target = {
  id: "", name: "", detail: "", icon: "ti-point", builtin: false,
  detect: { type: "path_exists", path: "" },
  writer: { format: "key_value", path: "", entries: [{ section: "", key: "", value: "http://{proxy}" }] },
};
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
  gradle: { name: "Gradle", detail: "Stacker 的 Gradle 初始化脚本中的代理", icon: "ti-hammer" },
  gradle_props: { name: "gradle.properties", detail: "~/.gradle/gradle.properties 的 systemProp.http(s).proxy", icon: "ti-file-settings" },
};

const ERRORS: Record<string, string> = {
  E_PROXY_ADDR: "Windows 没有开启系统代理，没有可写入的地址。",
  E_EXTERNAL_FILE: "settings.xml 是你自己维护的文件，Stacker 不会改写它，请手动编辑。",
  E_NOT_INSTALLED: "这台机器上没有这个程序，Stacker 不会替它创建配置。",
  E_BROKEN_JSON: "这个 JSON 文件读不通，Stacker 不会覆盖它，请先修好。",
  E_TARGET_ID: "标识只能用字母、数字、- 和 _，且不能为空。",
  E_TARGET_BUILTIN: "这个标识是内置项占用的，请换一个。",
  E_TARGET_FIELDS: "名称和文件路径都要填。",
  E_TARGET_PATH: "只能写你自己用户目录里的文件。",
};
const errorText = (e: unknown) => ERRORS[String(e)] ?? String(e);

/** A name the user typed becomes an id Stacker can store it under. */
function slug(name: string) {
  const ascii = name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  return ascii || `target-${Date.now().toString(36)}`;
}

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
  const [draft, setDraft] = useState<Target | null>(null);
  const [probeUrl, setProbeUrl] = useState("https://api.openai.com");
  const [diagnosing, setDiagnosing] = useState(false);
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
  const saveTarget = (target: Target) => run("draft", () => invoke("proxy_target_save", { target }), `已保存 ${target.name}`);
  const removeTarget = (id: string) => run(id, () => invoke("proxy_target_remove", { id }), "已删除这个自定义项");
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
            const meta = LOCATION_INFO[row.id]
              ?? (row.target ? { name: row.target.name, detail: row.target.detail, icon: row.target.icon } : null)
              ?? { name: row.id, detail: "", icon: "ti-point" };
            const missing = row.installed === false;
            const state = missing
              ? { label: "未安装", cls: "n" as const, stale: false }
              : rowState(row.value, system);
            const service = row.id === "winhttp";
            const writable = info.on && !missing && (service ? report.service.known : true);
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
              {row.target && !row.target.builtin && <button className="gh sm" title="删除这个自定义项"
                disabled={!!busy} onClick={() => void removeTarget(row.target!.id)}>
                <i className="ti ti-trash" />
              </button>}
            </div>;
          })}
        </div>
        <div className="proxy-add">
          <button className="gh sm" disabled={!!busy} onClick={() => setDraft({ ...BLANK_TARGET })}>
            <i className="ti ti-plus" /> 添加其他程序
          </button>
          <span className="s dim">还有别的工具要走代理？描述它的配置文件，写入和撤销就和上面一样。</span>
        </div>
        <div className="proxy-diag">
          <i className="ti ti-plug-connected-x" />
          <span>连不上？</span>
          <input className="ip" value={probeUrl} placeholder="https://api.openai.com" onChange={(e) => setProbeUrl(e.target.value)} />
          <AiButton label="测一下再让 AI 诊断" title="先直连和经系统代理各访问一次这个地址，再把结果和上面各处状态交给 AI"
            disabled={!probeUrl.trim()} onClick={() => setDiagnosing(true)} />
        </div>
      </div>
      {diagnosing && <AiAskModal title="连不上在哪卡住" sub={probeUrl}
        note="先直连、再经系统代理各访问一次这个地址；把两次结果和本页的代理状态发给 AI，不含任何密钥。"
        run={async () => {
          const probe = await invoke("proxy_probe", { url: probeUrl });
          return askAi("proxy", {
            system: { state: system.state, server: system.server },
            service: report.service.known ? (report.service.server || "直连") : "未知",
            places: rows.map((row) => ({ name: (LOCATION_INFO[row.id] ?? row.target ?? { name: row.id }).name, value: row.value, installed: row.installed !== false })),
            probe,
          });
        }}
        onClose={() => setDiagnosing(false)} />}

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

      {draft && <Modal title={draft.id && !draft.builtin ? "编辑自定义程序" : "添加其他程序"} icon="ti-plus"
        sub="Stacker 只会改你在这里点名的键，文件里别的内容原样保留。"
        onClose={() => setDraft(null)}
        footer={<>
          <button className="gh sm" onClick={() => setDraft(null)}>取消</button>
          <button className="pr sm" disabled={!!busy || !draft.name.trim() || !draft.writer.path.trim() || !draft.writer.entries[0]?.key.trim()}
            onClick={() => { const target = { ...draft, id: draft.id.trim() || slug(draft.name) }; setDraft(null); void saveTarget(target); }}>
            <i className="ti ti-device-floppy" /> 保存
          </button>
        </>}>
        <div className="proxy-form">
          <label><span>名称</span>
            <input className="ip full" value={draft.name} placeholder="例如：Bun"
              onChange={(e) => setDraft({ ...draft, name: e.target.value })} /></label>
          <label><span>说明</span>
            <input className="ip full" value={draft.detail} placeholder="写给自己看的一句话，可留空"
              onChange={(e) => setDraft({ ...draft, detail: e.target.value })} /></label>
          <label><span>怎么算装了</span>
            <Select width={150} value={draft.detect.type} options={[
              { value: "path_exists", label: "某个路径存在" },
              { value: "command_on_path", label: "某个命令在 PATH 上" },
            ]} onChange={(v) => setDraft({ ...draft, detect: v === "path_exists" ? { type: "path_exists", path: "" } : { type: "command_on_path", command: "" } })} />
            <input className="ip full" placeholder={draft.detect.type === "path_exists" ? "~/.bun" : "bun"}
              value={draft.detect.type === "path_exists" ? draft.detect.path : draft.detect.command}
              onChange={(e) => setDraft({ ...draft, detect: draft.detect.type === "path_exists"
                ? { type: "path_exists", path: e.target.value }
                : { type: "command_on_path", command: e.target.value } })} />
          </label>
          <label><span>配置文件</span>
            <Select width={230} value={draft.writer.format} options={FORMATS.map((f) => ({ value: f.value, label: f.label }))}
              onChange={(v) => setDraft({ ...draft, writer: { ...draft.writer, format: v as Writer["format"] } })} />
            <input className="ip full" value={draft.writer.path} placeholder="~/.bunfig.toml 或 %APPDATA%/Tool/config.json"
              onChange={(e) => setDraft({ ...draft, writer: { ...draft.writer, path: e.target.value } })} /></label>
          {draft.writer.entries.map((item, index) => <label key={index}><span>{index === 0 ? "写入的键" : ""}</span>
            {draft.writer.format === "ini" && <input className="ip" style={{ width: 110 }} value={item.section} placeholder="节，如 global"
              onChange={(e) => setDraft({ ...draft, writer: { ...draft.writer, entries: draft.writer.entries.map((e2, i) => i === index ? { ...e2, section: e.target.value } : e2) } })} />}
            <input className="ip" style={{ width: 170 }} value={item.key} placeholder={draft.writer.format === "json" ? "/http.proxy" : "proxy"}
              onChange={(e) => setDraft({ ...draft, writer: { ...draft.writer, entries: draft.writer.entries.map((e2, i) => i === index ? { ...e2, key: e.target.value } : e2) } })} />
            <input className="ip full" value={item.value} placeholder="http://{proxy}"
              onChange={(e) => setDraft({ ...draft, writer: { ...draft.writer, entries: draft.writer.entries.map((e2, i) => i === index ? { ...e2, value: e.target.value } : e2) } })} />
            {draft.writer.entries.length > 1 && <button className="ic danger" title="删除这一行"
              onClick={() => setDraft({ ...draft, writer: { ...draft.writer, entries: draft.writer.entries.filter((_, i) => i !== index) } })}><i className="ti ti-trash" /></button>}
          </label>)}
          <div className="proxy-form-foot">
            <button className="gh sm" onClick={() => setDraft({ ...draft, writer: { ...draft.writer, entries: [...draft.writer.entries, { section: draft.writer.entries[0]?.section ?? "", key: "", value: "http://{proxy}" }] } })}>
              <i className="ti ti-plus" /> 再加一个键
            </button>
            <span className="s dim">值里的 {"{proxy}"} 会换成系统代理的 host:port。</span>
          </div>
        </div>
      </Modal>}

      {confirmRelease && <ConfirmModal title="撤销所有位置的代理" icon="ti-eraser" danger
        message="系统代理已经关闭，下面仍写着代理的位置都会被清除，包括不是 Stacker 写入的。确定吗？"
        confirmLabel="全部撤销"
        onConfirm={() => { setConfirmRelease(false); void releaseAll(); }}
        onClose={() => setConfirmRelease(false)} />}
    </>
  );
}
