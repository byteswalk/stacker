import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { Modal, useToast } from "../../ui";
import { formatSpaceBytes as bytes } from "../space-analysis/components/SpaceOverview";

type TransferProject = { path: string; name: string; exists: boolean; sessions: number };
type ExportResult = { sessions: number; skipped: number; projects: number; files: number; bytes: number };
type AccountCheck = { agent: string; same: boolean; packed: boolean; here: boolean };
type PreviewSession = { agent: "codex" | "claude"; id: string; title: string; cwd: string; updatedAt: number; present: boolean };
type PreviewProject = { path: string; name: string; included: boolean; files: number; bytes: number; existsHere: boolean };
type Preview = { machine: string; createdAt: number; sessions: PreviewSession[]; projects: PreviewProject[]; accounts: AccountCheck[]; codexRunning: boolean; codexReady: boolean };
type Imported = { agent: "codex" | "claude"; id: string; title: string; cwd: string; resume: string };
type ImportResult = { imported: Imported[]; present: string[]; stripped: string[]; dropped: number; extracted: number; kept: number; backup: string | null };
type Target = { from: string; to: string; extract: boolean };

const AGENT: Record<string, string> = { codex: "Codex", claude: "Claude Code" };
const ERRORS: Record<string, string> = {
  E_CODEX_RUNNING: "Codex 还在运行：请完全退出 Codex 桌面版和命令行后再导入。",
  E_CODEX_NOT_READY: "这台电脑还没用过 Codex：先打开 Codex 登录一次，然后完全退出，再导入。",
  E_TRANSFER_FORMAT: "这不是 Stacker 导出的会话迁移包。",
};
const errorText = (error: unknown, tr: (text: string) => string) => {
  const text = String(error);
  if (text.startsWith("E_CODEX_SCHEMA:")) return tr("这台电脑的 Codex 版本和迁移包不兼容（缺少字段 {field}），请把两边的 Codex 都更新到最新再试。").replace("{field}", text.slice(15));
  return tr(ERRORS[text] ?? text);
};

function today(): string {
  const now = new Date();
  return `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, "0")}${String(now.getDate()).padStart(2, "0")}`;
}

/** How far packing or unpacking has got: `[stage, done, total]` while `active`. */
function useTransferProgress(active: boolean): [string, number, number] | null {
  const [progress, setProgress] = useState<[string, number, number] | null>(null);
  useEffect(() => {
    setProgress(null);
    if (!active) return;
    let stop: (() => void) | undefined;
    let disposed = false;
    void listen<[string, number, number]>("sessions-transfer-progress", (event) => { if (!disposed) setProgress(event.payload); })
      .then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; });
    return () => { disposed = true; stop?.(); };
  }, [active]);
  return progress;
}

function ProgressLine({ progress }: { progress: [string, number, number] | null }) {
  const { tr } = useI18n();
  if (!progress) return <span>{tr("正在处理…")}</span>;
  const [stage, done, total] = progress;
  const what = stage === "project" ? "工程文件" : stage === "codex" ? "Codex 会话" : stage === "claude" ? "Claude Code 会话" : "会话";
  return <span>{tr("正在处理{what} {done}/{total}").replace("{what}", tr(what)).replace("{done}", String(done)).replace("{total}", String(total))}</span>;
}

/** Packs the chosen sessions (and, if asked, their project folders) for another computer. */
export function TransferExport({ ids, onClose }: { ids: string[]; onClose: () => void }) {
  const { tr } = useI18n();
  const toast = useToast();
  const [projects, setProjects] = useState<TransferProject[] | null>(null);
  const [include, setInclude] = useState<Set<string>>(new Set());
  const [sizes, setSizes] = useState<Record<string, [number, number] | "counting">>({});
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<(ExportResult & { path: string }) | null>(null);
  const progress = useTransferProgress(busy);

  useEffect(() => {
    let alive = true;
    invoke<TransferProject[]>("sessions_transfer_projects", { ids }).then((list) => { if (alive) setProjects(list); }).catch((e) => { if (alive) { setProjects([]); toast(errorText(e, tr), "err"); } });
    return () => { alive = false; };
  }, [ids, toast, tr]);

  function toggle(path: string, on: boolean) {
    setInclude((old) => { const next = new Set(old); if (on) next.add(path); else next.delete(path); return next; });
    if (on && !sizes[path]) {
      setSizes((old) => ({ ...old, [path]: "counting" }));
      void invoke<[number, number]>("sessions_transfer_size", { path }).then((size) => setSizes((old) => ({ ...old, [path]: size })));
    }
  }

  async function run() {
    const dest = await save({ title: tr("保存会话迁移包"), defaultPath: `Stacker-会话迁移-${today()}.zip`, filters: [{ name: tr("会话迁移包"), extensions: ["zip"] }] });
    if (!dest) return;
    setBusy(true);
    try {
      const result = await invoke<ExportResult>("sessions_transfer_export", { ids, include: [...include], dest });
      setDone({ ...result, path: dest });
    } catch (e) { toast(errorText(e, tr), "err"); }
    finally { setBusy(false); }
  }

  const supported = projects?.reduce((sum, p) => sum + p.sessions, 0) ?? 0;
  return <Modal wide title="迁移到另一台电脑" icon="ti-transfer" onClose={busy ? undefined : onClose}
    footer={done
      ? <button className="pr sm" onClick={onClose}>完成</button>
      : <>
        <button className="gh sm" disabled={busy} onClick={onClose}>取消</button>
        <button className="pr sm" disabled={busy || !supported} onClick={() => void run()}>
          {busy ? <><i className="ti ti-loader spin" /> <ProgressLine progress={progress} /></> : <><i className="ti ti-package-export" /> {tr("选择位置并打包")}</>}
        </button>
      </>}>
    {done ? <div className="vault-form">
      <div className="callout" style={{ margin: 0 }}><i className="ti ti-circle-check" /><div>
        {tr("已打包 {sessions} 个会话、{projects} 个工程目录，迁移包 {size}。").replace("{sessions}", String(done.sessions)).replace("{projects}", String(done.projects)).replace("{size}", bytes(done.bytes))}
        {done.skipped > 0 && tr("另有 {count} 个其他智能体的会话暂不支持迁移，没有打包。").replace("{count}", String(done.skipped))}
      </div></div>
      <div className="transfer-steps">
        <b>{tr("在另一台电脑上")}</b>
        <ol>
          <li>{tr("装好 Stacker 和对应的智能体（Codex 要打开一次并登录，然后完全退出）。")}</li>
          <li>{tr("把这个迁移包拷过去，在“会话数据”页右上角点“导入迁移包”。")}</li>
          <li>{tr("给每个工程指定那台电脑上的位置，导入后按提示的命令接着聊。")}</li>
        </ol>
      </div>
      <div className="vault-warn">{tr("迁移包里有对话原文，带了工程目录的还有源码和其中的配置，请私下传输，不要公开上传。")}</div>
      <div><button className="gh sm" onClick={() => void invoke("space_open_directory", { path: done.path.replace(/[\\/][^\\/]+$/, "") }).catch((e) => toast(String(e), "err"))}><i className="ti ti-folder-open" /> {tr("打开文件所在位置")}</button></div>
    </div> : <div className="vault-form">
      <div className="vault-sub" style={{ margin: 0 }}>{tr("把选中的会话打成一个迁移包，拷到另一台电脑导入后可以接着聊。目前支持 Codex 和 Claude Code，其他智能体的会话会被跳过。")}</div>
      {projects === null ? <div className="vault-empty">{tr("正在读取…")}</div> : projects.length === 0
        ? <div className="vault-warn">{tr("选中的会话里没有 Codex 或 Claude Code 的会话。")}</div>
        : <>
          <div className="transfer-head">{tr("{count} 个会话，涉及 {projects} 个工程目录").replace("{count}", String(supported)).replace("{projects}", String(projects.length))}</div>
          <div className="transfer-projects">
            {projects.map((project) => {
              const size = sizes[project.path];
              return <label key={project.path} className={"transfer-project" + (project.exists ? "" : " missing")}>
                <input type="checkbox" checked={include.has(project.path)} disabled={!project.exists || busy} onChange={(e) => toggle(project.path, e.target.checked)} />
                <span className="mt">
                  <b translate="no">{project.name || project.path}</b>
                  <small translate="no" title={project.path}>{project.path}</small>
                </span>
                <span className="mut">{!project.exists ? tr("目录已不在") : include.has(project.path)
                  ? size === "counting" || !size ? tr("正在统计…") : tr("{files} 个文件 · {size}").replace("{files}", String(size[0])).replace("{size}", bytes(size[1]))
                  : tr("{count} 个会话").replace("{count}", String(project.sessions))}</span>
              </label>;
            })}
          </div>
          <div className="vault-sub" style={{ margin: 0 }}>{tr("勾选的工程目录会一起打包，包括 .git 和隐藏文件；node_modules、target、.venv 这类能重新生成的目录不打包。工程在 Git 上的，可以不勾，到那台电脑上直接拉代码。")}</div>
        </>}
    </div>}
  </Modal>;
}

/** Brings a package's sessions (and project folders) into this computer's Codex and Claude Code. */
export function TransferImport({ onClose }: { onClose: (changed: boolean) => void }) {
  const { tr } = useI18n();
  const toast = useToast();
  const [path, setPath] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [targets, setTargets] = useState<Target[]>([]);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<ImportResult | null>(null);
  const progress = useTransferProgress(busy && !!preview);

  async function read(file: string) {
    setBusy(true);
    try {
      const next = await invoke<Preview>("sessions_transfer_preview", { path: file });
      setPreview(next);
      setTargets((old) => next.projects.map((p) => old.find((t) => t.from === p.path) ?? { from: p.path, to: p.path, extract: p.included }));
    } catch (e) { setPreview(null); toast(errorText(e, tr), "err"); }
    finally { setBusy(false); }
  }
  async function choose() {
    const picked = await open({ title: tr("选择会话迁移包"), multiple: false, directory: false, filters: [{ name: tr("会话迁移包"), extensions: ["zip"] }] });
    if (typeof picked !== "string") return;
    setPath(picked);
    setDone(null);
    await read(picked);
  }
  async function pickFolder(index: number) {
    const picked = await open({ title: tr("选择工程在这台电脑上的位置"), directory: true, multiple: false });
    if (typeof picked === "string") setTargets((old) => old.map((t, i) => i === index ? { ...t, to: picked } : t));
  }
  async function run() {
    setBusy(true);
    try { setDone(await invoke<ImportResult>("sessions_transfer_import", { path, targets })); }
    catch (e) { toast(errorText(e, tr), "err"); if (String(e) === "E_CODEX_RUNNING") void read(path); }
    finally { setBusy(false); }
  }
  async function copy(text: string) {
    try { await navigator.clipboard.writeText(text); toast(tr("已复制"), "ok"); } catch { toast(tr("复制失败，请手动选中复制"), "err"); }
  }

  const hasCodex = !!preview?.sessions.some((s) => s.agent === "codex" && !s.present);
  const blocked = hasCodex && (preview!.codexRunning || !preview!.codexReady);
  const fresh = preview?.sessions.filter((s) => !s.present).length ?? 0;
  return <Modal wide title="导入会话迁移包" icon="ti-transfer-in" onClose={busy ? undefined : () => onClose(!!done?.imported.length)}
    footer={done
      ? <button className="pr sm" onClick={() => onClose(true)}>完成</button>
      : <>
        <button className="gh sm" disabled={busy} onClick={() => onClose(false)}>取消</button>
        {preview && hasCodex && preview.codexRunning && <button className="gh sm" disabled={busy} onClick={() => void read(path)}><i className="ti ti-refresh" /> {tr("我已退出 Codex，重新检查")}</button>}
        <button className="pr sm" disabled={busy || !preview || !fresh || blocked || targets.some((t) => !t.to.trim())} onClick={() => void run()}>
          {busy && preview ? <><i className="ti ti-loader spin" /> <ProgressLine progress={progress} /></> : <><i className="ti ti-package-import" /> {tr("导入 {count} 个会话").replace("{count}", String(fresh))}</>}
        </button>
      </>}>
    {done ? <div className="vault-form">
      <div className="callout" style={{ margin: 0 }}><i className="ti ti-circle-check" /><div>
        {tr("已导入 {count} 个会话。").replace("{count}", String(done.imported.length))}
        {done.present.length > 0 && tr("{count} 个这台电脑上已经有了，没有改动。").replace("{count}", String(done.present.length))}
        {done.extracted > 0 && tr("解压了 {count} 个工程文件").replace("{count}", String(done.extracted))}
        {done.kept > 0 && tr("（{count} 个已存在的文件保持原样）").replace("{count}", String(done.kept))}
      </div></div>
      {done.stripped.length > 0 && <div className="vault-sub" style={{ margin: 0 }}>
        {tr("{agents} 在这台电脑登录的是另一个账号，已去掉只有原账号能用的加密推理和思考签名（{count} 处），对话正文完整保留。").replace("{agents}", done.stripped.map((a) => AGENT[a] ?? a).join("、")).replace("{count}", String(done.dropped))}
      </div>}
      {done.imported.length > 0 && <div className="transfer-resume">
        <b>{tr("接着聊")}</b>
        {done.imported.map((item) => <div key={item.agent + item.id}>
          <span className="mt"><b translate="no">{item.title}</b><small>{AGENT[item.agent]} · <span translate="no">{item.cwd}</span></small></span>
          <code translate="no">{item.resume}</code>
          <button className="gh xs" title={tr("复制命令")} onClick={() => void copy(item.resume)}><i className="ti ti-copy" /></button>
        </div>)}
      </div>}
      {done.backup && <div className="vault-sub" style={{ margin: 0 }} translate="no">{tr("导入前已备份 Codex 的会话数据库：{path}").replace("{path}", done.backup)}</div>}
    </div> : <div className="vault-form">
      <div className="vault-bar" style={{ margin: 0 }}>
        <button className="gh sm" disabled={busy} onClick={() => void choose()}><i className={"ti " + (busy && !preview ? "ti-loader spin" : "ti-folder-open")} /> {tr("选择迁移包")}</button>
        <span className="mut grow" translate="no">{path}</span>
      </div>
      {!preview && <div className="vault-sub" style={{ margin: 0 }}>{tr("选择另一台电脑上用“迁移…”打出来的 .zip 迁移包。")}</div>}
      {preview && <>
        <div className="vault-sub" style={{ margin: 0 }}>{tr("来自 {machine}，打包于 {time}").replace("{machine}", preview.machine || tr("另一台电脑")).replace("{time}", new Date(preview.createdAt * 1000).toLocaleString())}</div>
        {preview.accounts.map((account) => <div key={account.agent} className={"transfer-account" + (account.same ? " same" : "")}>
          <i className={"ti " + (account.same ? "ti-user-check" : "ti-user-question")} />
          <span><b>{AGENT[account.agent] ?? account.agent}</b>：{account.same
            ? tr("两台电脑登录的是同一个账号，完整导入。")
            : !account.here
              ? tr("这台电脑还没登录，按不同账号处理：去掉只有原账号能用的加密推理和思考签名，对话正文完整保留。")
              : tr("两台电脑登录的不是同一个账号：去掉只有原账号能用的加密推理和思考签名，对话正文完整保留。")}</span>
        </div>)}
        {hasCodex && preview.codexRunning && <div className="vault-warn">{tr("Codex 还在运行。导入要写 Codex 的会话数据库，请先完全退出 Codex 桌面版和命令行，再点“我已退出 Codex，重新检查”。")}</div>}
        {hasCodex && !preview.codexReady && <div className="vault-warn">{tr("这台电脑还没用过 Codex：先打开 Codex 登录一次，然后完全退出，再导入。")}</div>}
        <div className="transfer-head">{tr("工程位置")}</div>
        <div className="transfer-targets">
          {preview.projects.map((project, index) => {
            const target = targets[index];
            if (!target) return null;
            return <div key={project.path} className="transfer-target">
              <div className="from"><b translate="no">{project.name || project.path}</b><small translate="no" title={project.path}>{tr("原位置")} {project.path}</small></div>
              <div className="to">
                <input className="ip" value={target.to} onChange={(e) => setTargets((old) => old.map((t, i) => i === index ? { ...t, to: e.target.value } : t))} />
                <button className="gh sm" title={tr("选择文件夹")} onClick={() => void pickFolder(index)}><i className="ti ti-folder-open" /></button>
              </div>
              {project.included
                ? <label className="vault-check"><input type="checkbox" checked={target.extract} onChange={(e) => setTargets((old) => old.map((t, i) => i === index ? { ...t, extract: e.target.checked } : t))} />
                  <span>{tr("把包里的工程文件解压到这里（{files} 个文件 · {size}，已有的文件保持原样）").replace("{files}", String(project.files)).replace("{size}", bytes(project.bytes))}</span></label>
                : <small className="mut">{tr("迁移包里没带这个工程的文件：请先把工程放到上面的位置（比如从 Git 拉下来）。")}</small>}
            </div>;
          })}
        </div>
        <div className="transfer-head">{tr("会话")}</div>
        <div className="transfer-sessions">
          {preview.sessions.map((session) => <div key={session.agent + session.id} className={session.present ? "present" : ""}>
            <span className="agent">{AGENT[session.agent]}</span>
            <b translate="no" title={session.title}>{session.title}</b>
            {session.present && <span className="bd">{tr("这台电脑已有")}</span>}
          </div>)}
        </div>
      </>}
    </div>}
  </Modal>;
}
