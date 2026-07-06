import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Page } from "../App";
import { Modal, useToast } from "../ui";

type Mirror = { id: string; name: string; url: string; host: string };
type ToolState = {
  id: string; name: string; icon: string; config: string;
  installed: boolean; current: string | null; current_label: string; mirrors: Mirror[];
};

// 工具 → 生态页
const TOOL_ECO: Record<string, Page> = {
  pip: "python", conda: "python", npm: "node", yarn: "node",
  go: "go", maven: "maven", gradle: "gradle", cargo: "rust",
};
type CheckItem = { id: string; sev: string; title: string; desc: string; page: Page; action: string };
function firstMirror(t: ToolState) { return t.mirrors.find((m) => m.id !== "official"); }
function onOfficial(t: ToolState) { return !["python-runtime", "node-runtime"].includes(t.id) && t.installed && (t.current === "official" || !t.current) && !!firstMirror(t); }

// 可由 Stacker 直接修复的 extra 项 → 返回执行函数（成功时给提示语）；null = 只跳页处理（如 Java 对齐需 UAC + 选方向）。
function extraFixer(id: string): null | (() => Promise<string>) {
  switch (id) {
    case "fnm_no_integration":
      return async () => { await invoke("fnm_write_integration", { shells: ["powershell", "gitbash", "cmd"] }); return "已写入 fnm shell 集成（新终端生效）"; };
    case "proxy_stale":
      return async () => { await invoke("proxy_disable", { alsoJvm: false }); return "已关闭终端代理"; };
    case "cache_high":
      return async () => { const freed = await invoke<number>("cleanup_delete_safe"); return `已清理安全缓存，释放 ${(freed / 1073741824).toFixed(1)} GB`; };
    default:
      return null;
  }
}
// 纳入「一键优化全部」的 extra 项：仅安全、可还原、免提权的（fnm 集成）；
// 缓存清理（删除）、关代理这些副作用项只给各自的行内按钮，不卷进批量。
const BATCH_EXTRA = new Set(["fnm_no_integration"]);

/* ── 浏览器无后端时的演示数据 ── */
type DemoFix = { sev: "warn" | "mid" | "info"; title: string; badge: [string, string]; desc: ReactNode; action: string; go?: Page };
const DEMO_FIXES: DemoFix[] = [
  { sev: "warn", title: "Node 的 fnm 集成缺 cmd", badge: ["r", "影响生效"], desc: <>cmd 未写入集成 → 切版本不生效</>, action: "补全", go: "node" },
  { sev: "mid", title: "pip 仍在用官方源", badge: ["w", "建议"], desc: <>下载慢；切到最快国内源</>, action: "切最快源", go: "python" },
  { sev: "info", title: "C 盘开发缓存偏高", badge: ["b", "提示"], desc: <>各 cache 共占 C 盘 12.6 GB，可安全释放约 9 GB</>, action: "去清理", go: "cleanup" },
];

export default function Overview({ goto }: { goto: (p: Page) => void }) {
  const toast = useToast();
  const [tools, setTools] = useState<ToolState[] | null>(null);
  const [extra, setExtra] = useState<CheckItem[]>([]);
  const [demo, setDemo] = useState(false);
  const [busy, setBusy] = useState(false);
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<{ tool: string; from: string; to: string }[] | null>(null);

  async function load() {
    try {
      setTools(await invoke<ToolState[]>("list_sources")); setDemo(false);
      setChecking(true);
      try { setExtra(await invoke<CheckItem[]>("checkup_extra")); } catch { setExtra([]); } finally { setChecking(false); }
    } catch { setDemo(true); setTools([]); setChecking(false); }
  }
  useEffect(() => { load(); }, []);

  const realFixes = (tools ?? []).filter(onOfficial);
  const batchExtra = extra.filter((e) => BATCH_EXTRA.has(e.id));
  const optimizeCount = realFixes.length + batchExtra.length;
  const installedCount = (tools ?? []).filter((t) => !["python-runtime", "node-runtime"].includes(t.id) && t.installed).length;
  const emptySetup = !demo && tools !== null && installedCount === 0 && extra.length === 0;
  const count = emptySetup ? 0 : demo ? DEMO_FIXES.length : realFixes.length + extra.length;
  const allOk = !demo && !emptySetup && count === 0;
  // 标题如实拆分：换源类（可一键优化）vs 环境/缓存类（各自单独处理）
  const subtitle = (() => {
    const parts: string[] = [];
    if (realFixes.length) parts.push(`${realFixes.length} 个包管理器仍在官方源`);
    if (extra.length) parts.push(`${extra.length} 项环境 / 缓存可优化`);
    return parts.join(" · ") || "检测到可优化项";
  })();

  async function applyTool(t: ToolState) {
    const m = firstMirror(t)!;
    await invoke("apply_source", { toolId: t.id, mirrorId: m.id });
    return { tool: t.name, from: t.current_label || "官方", to: m.name };
  }
  async function fixOne(t: ToolState) {
    setBusy(true);
    try { const r = await applyTool(t); await load(); toast(`已切换 ${r.tool} → ${r.to}`, "ok"); }
    catch (e) { toast("切换失败：" + e, "err"); } finally { setBusy(false); }
  }
  // 单个 extra 项的行内一键修复（fnm 集成 / 关代理 / 清缓存）
  async function runExtra(fixer: () => Promise<string>) {
    setBusy(true);
    try { const msg = await fixer(); await load(); toast(msg, "ok"); }
    catch (err) { toast("操作失败：" + err, "err"); } finally { setBusy(false); }
  }
  async function optimizeAll() {
    if (demo) { setResult([{ tool: "pip", from: "官方", to: "清华" }, { tool: "npm", from: "官方", to: "npmmirror" }]); return; }
    setBusy(true);
    const done: { tool: string; from: string; to: string }[] = [];
    try {
      for (const t of realFixes) done.push(await applyTool(t));
      // 批量里的环境项（仅安全可还原的，如 fnm 集成）
      for (const e of batchExtra) {
        const f = extraFixer(e.id);
        if (f) { await f(); done.push({ tool: "fnm shell 集成", from: "未写入", to: "已写入" }); }
      }
      await load(); setResult(done);
      toast(`已优化 ${done.length} 项`, "ok");
    } catch (e) { toast("优化失败：" + e, "err"); } finally { setBusy(false); }
  }

  return (
    <>
      <div className={"checkup" + (allOk ? " ok" : "")}>
        <span className="cnum">{allOk ? <i className="ti ti-circle-check" style={{ fontSize: 26 }} /> : emptySetup ? <i className="ti ti-package-off" style={{ fontSize: 26 }} /> : <><b>{count}</b><span>可优化</span></>}</span>
        <div className="ct">
          <div className="t1">{emptySetup ? "未检测到开发工具" : allOk ? "一切就绪" : "开发环境体检"}</div>
          <div className="t2">{checking ? "正在体检：检测 Python / Node / 代理 / 缓存 / Java 版本一致性…"
            : demo ? "演示数据（在 Tauri 应用内运行可读取真实状态）"
            : emptySetup ? "这像是一台新机器：先进入左侧 Python / Node / Java 等页面安装运行时，再配置对应包源。"
            : allOk ? "已安装的包管理器都已完成配置，未发现需要立即处理的问题" : subtitle}</div>
        </div>
        <div className="cacts">
          <button className="gh sm" disabled={checking} onClick={() => load().then(() => toast("已重新体检", "ok"))}><i className={"ti " + (checking ? "ti-loader spin" : "ti-refresh")} /> {checking ? "体检中…" : "重新体检"}</button>
          {!demo && optimizeCount > 0 && <button className="pr" disabled={busy} onClick={optimizeAll}><i className="ti ti-wand" /> {busy ? "优化中…" : `一键优化（${optimizeCount}）`}</button>}
        </div>
      </div>

      {!allOk && !emptySetup && <div className="seclabel"><i className="ti ti-list-check" /> 可优化项</div>}
      {demo
        ? DEMO_FIXES.map((f, i) => (
          <div className="fixrow" key={i}>
            <span className={"fdot " + f.sev} />
            <div className="ft"><div className="fh">{f.title} <span className={"bd " + f.badge[0]}>{f.badge[1]}</span></div><div className="fs">{f.desc}</div></div>
            <button className={f.sev === "info" ? "gh sm" : "pr sm"} onClick={() => f.go && goto(f.go)}>{f.action}</button>
          </div>
        ))
        : realFixes.map((t) => (
          <div className="fixrow" key={t.id}>
            <span className="fdot mid" />
            <div className="ft"><div className="fh">{t.name} 仍在用官方源 <span className="bd w">建议</span></div>
              <div className="fs">当前：{t.current_label || "官方"} → 切到国内源更快（{firstMirror(t)?.name}）</div></div>
            <button className="pr sm" disabled={busy} onClick={() => fixOne(t)}>切到国内源</button>
          </div>
        ))}

      {!demo && extra.map((e) => {
        const fixer = extraFixer(e.id);
        return (
          <div className="fixrow" key={e.id}>
            <span className={"fdot " + e.sev} />
            <div className="ft">
              <div className="fh">{e.title} <span className={"bd " + (e.sev === "warn" ? "r" : e.sev === "mid" ? "w" : "b")}>{e.sev === "warn" ? "注意" : e.sev === "mid" ? "建议" : "提示"}</span></div>
              <div className="fs">{e.desc}</div>
            </div>
            <button className={e.sev === "info" ? "gh sm" : "pr sm"} disabled={busy}
              onClick={fixer ? () => runExtra(fixer) : () => goto(e.page)}>{e.action}</button>
          </div>
        );
      })}

      <div className="seclabel"><i className="ti ti-stack-2" /> 生态总览</div>
      {(["python", "node", "go", "maven", "gradle", "rust"] as Page[]).map((eco) => {
        const t = (tools ?? []).find((x) => TOOL_ECO[x.id] === eco && x.installed);
        const meta = ECO_META[eco];
        const pageIssue = extra.find((e) => e.page === eco && e.sev !== "info");
        const statusText = demo ? "正常" : pageIssue ? "需处理" : !t ? "—" : onOfficial(t) ? "可优化" : "正常";
        const statusColor = demo ? "#6bcf86" : pageIssue ? "#ef6f6f" : !t ? "#828995" : onOfficial(t) ? "#e4b450" : "#6bcf86";
        return (
          <div className="ecorow" key={eco} onClick={() => goto(eco)}>
            <span className={"av " + meta.av + " big"}><i className={"ti " + meta.icon} /></span>
            <div className="ecocols">
              <div className="ecocell"><div className="k">生态</div><div className="v">{meta.label}</div></div>
              <div className="ecocell"><div className="k">主包源</div><div className="v">{demo ? meta.demoSrc : t ? (t.current === "official" || !t.current ? <span style={{ color: "#e4b450" }}>官方（建议换源）</span> : <>{t.current_label} <span className="live" style={{ fontSize: 11 }}><i className="ti ti-circle-check" /></span></>) : <span style={{ color: "#828995" }}>未检测到</span>}</div></div>
              <div className="ecocell"><div className="k">状态</div><div className="v" style={{ color: statusColor }}>{statusText}</div></div>
            </div>
            <i className="ti ti-chevron-right chev" />
          </div>
        );
      })}

      {result && (
        <Modal title={`优化完成 · ${result.length} 项`} icon="ti-circle-check" onClose={() => setResult(null)}
          sub={<span>改动已自动备份，可在「历史」还原</span>}
          footer={<button className="pr sm" onClick={() => setResult(null)}>完成</button>}>
          {result.length === 0
            ? <div style={{ fontSize: 13, color: "var(--tx)" }}>没有需要切换的源，全部已是国内源。</div>
            : <div style={{ display: "flex", flexDirection: "column", gap: 7 }}>
              {result.map((r, i) => (
                <div className="vrow" key={i} style={{ margin: 0 }}>
                  <span className="ver" style={{ minWidth: 88, fontFamily: "inherit", fontWeight: 400 }}>{r.tool}</span>
                  <span className="meta">{r.from} → <b style={{ color: "var(--tx)" }}>{r.to}</b></span>
                  <span className="ntag ok">已应用</span>
                </div>
              ))}
            </div>}
        </Modal>
      )}
    </>
  );
}

const ECO_META: Record<string, { av: string; icon: string; label: string; demoSrc: ReactNode }> = {
  python: { av: "py", icon: "ti-brand-python", label: "Python · pip", demoSrc: <span style={{ color: "#e4b450" }}>官方（建议换源）</span> },
  node: { av: "npm", icon: "ti-brand-nodejs", label: "Node · npm", demoSrc: <>npmmirror</> },
  go: { av: "go", icon: "ti-brand-golang", label: "Go · GOPROXY", demoSrc: <>goproxy.cn</> },
  maven: { av: "mv2", icon: "ti-feather", label: "Maven", demoSrc: <>阿里云</> },
  gradle: { av: "gr", icon: "ti-box", label: "Gradle", demoSrc: <>阿里云</> },
  rust: { av: "rs", icon: "ti-brand-rust", label: "Rust · Cargo", demoSrc: <>字节 rsproxy</> },
};
