import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "../../invoke";
import { translateText } from "../../i18n";
import { ConfirmModal, Modal, operationWasCancelled, useBusy, useToast } from "../../ui";
import { Select } from "../../Select";
import { kindName, venvSummary, type VenvInfo } from "./venvSummary";

const PROJECTS_KEY = "stacker.python.venvProjects";

type DriveInfo = { letter: string; fixed: boolean };
type PipMirror = { id: string; name: string };
type RebuildResult = { info: VenvInfo; notes: string[] };
type Version = { version: string; path?: string | null; isDefault: boolean };

function loadProjects(): string[] {
  try {
    const list = JSON.parse(localStorage.getItem(PROJECTS_KEY) ?? "[]");
    return Array.isArray(list) ? list.filter((p) => typeof p === "string") : [];
  } catch {
    return [];
  }
}

function saveProjects(list: string[]) {
  try {
    localStorage.setItem(PROJECTS_KEY, JSON.stringify(list));
  } catch {
    // The list is a convenience; the environments themselves live in the projects.
  }
}

function folderName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** Project environments: find, create, rebuild and configure a project's own Python. */
export function VenvPanel({ versions, openFolder }: { versions: Version[]; openFolder: (path: string) => void }) {
  const toast = useToast();
  const runBusy = useBusy();
  const [projects, setProjects] = useState<string[]>(loadProjects);
  const [infos, setInfos] = useState<Record<string, VenvInfo>>({});
  const [pick, setPick] = useState<Record<string, string>>({});
  const [detail, setDetail] = useState<VenvInfo | null>(null);
  const [removing, setRemoving] = useState<VenvInfo | null>(null);
  const [rebuild, setRebuild] = useState<{ info: VenvInfo; version: string; keep: boolean } | null>(null);
  const [rebuilt, setRebuilt] = useState<RebuildResult | null>(null);
  const [pipMirrors, setPipMirrors] = useState<PipMirror[]>([]);
  const [pipPick, setPipPick] = useState("");
  const usable = versions.filter((v) => v.path);
  const fallback = (usable.find((v) => v.isDefault) ?? usable[0])?.version ?? "";

  async function inspect(project: string) {
    const info = await invoke<VenvInfo>("python_venv_inspect", { project });
    setInfos((cur) => ({ ...cur, [project]: info }));
    return info;
  }

  useEffect(() => {
    projects.forEach((p) => void inspect(p).catch(() => {}));
    invoke<{ mirrors: PipMirror[] }>("pip_config_state", { customPath: null })
      .then((state) => setPipMirrors(state.mirrors ?? []))
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function update(list: string[]) {
    setProjects(list);
    saveProjects(list);
  }

  function add(paths: string[]) {
    const known = new Set(projects.map((p) => p.toLowerCase()));
    const fresh = paths.filter((p) => !known.has(p.toLowerCase()));
    if (fresh.length) update([...fresh, ...projects]);
    return fresh.length;
  }

  async function addProject() {
    const dir = await open({ directory: true, multiple: false, title: translateText("选择项目文件夹") });
    if (!dir || typeof dir !== "string") return;
    add([dir]);
    const info = await inspect(dir);
    toast(info.dir ? `已找到${kindName(info.kind)}：${info.dir}` : "这个项目还没有虚拟环境，可以选版本创建", info.dir ? "ok" : "info");
  }

  async function scan() {
    try {
      const drives = await invoke<DriveInfo[]>("list_drives");
      const roots = drives.filter((d) => d.fixed).map((d) => `${d.letter}\\`);
      const found = await runBusy({
        title: "扫描本机项目环境",
        message: "正在扫描固定磁盘中的虚拟环境（pyvenv.cfg）和项目自带的嵌入式 Python。只读取，不改动任何文件。",
        progressEvent: "python-env-scan-progress",
        cancel: { label: "取消扫描", onCancel: () => invoke("python_env_scan_cancel").catch(() => undefined) },
      }, () => invoke<VenvInfo[]>("python_env_scan", { roots }));
      setInfos((cur) => {
        const next = { ...cur };
        found.forEach((info) => { next[info.project] = info; });
        return next;
      });
      const added = add(found.map((info) => info.project));
      toast(`扫描完成，发现 ${found.length} 个项目环境，新增 ${added} 个`, "ok");
    } catch (e) {
      if (!operationWasCancelled(e)) toast("扫描项目环境失败：" + e, "err");
    }
  }

  function interpreterOf(version: string) {
    const found = usable.find((v) => v.version === version);
    return found?.path ? `${found.path.replace(/[\\/]+$/, "")}\\python.exe` : "";
  }

  async function create(project: string) {
    const python = interpreterOf(pick[project] ?? fallback);
    if (!python) {
      toast("请先在上方安装一个 Python 版本", "info");
      return;
    }
    try {
      const info = await runBusy(
        { title: "创建虚拟环境", message: `正在用 Python ${pick[project] ?? fallback} 在 ${project} 下创建 .venv…` },
        () => invoke<VenvInfo>("python_venv_create", { project, python }),
      );
      setInfos((cur) => ({ ...cur, [project]: info }));
      toast(`已创建 .venv（Python ${info.version ?? pick[project] ?? fallback}）`, "ok");
    } catch (e) {
      toast("创建虚拟环境失败：" + e, "err");
    }
  }

  async function runRebuild() {
    if (!rebuild) return;
    const { info, version, keep } = rebuild;
    const python = interpreterOf(version);
    setRebuild(null);
    try {
      const result = await runBusy({
        title: "更换虚拟环境的 Python 版本",
        message: keep
          ? `正在记录已装的包，用 Python ${version} 重建 ${info.dir}，然后重新安装依赖。依赖较多时会比较久。`
          : `正在用 Python ${version} 重建 ${info.dir}。`,
      }, () => invoke<RebuildResult>("python_venv_rebuild", { dir: info.dir, python, keepPackages: keep }));
      // The rebuild reports the environment folder; the list is keyed by what the user added.
      const refreshed = await inspect(info.project);
      setRebuilt({ ...result, info: refreshed });
      setDetail(null);
    } catch (e) {
      toast("更换版本失败：" + e, "err");
    }
  }

  async function applyPipMirror(info: VenvInfo) {
    if (!pipPick || !info.dir) return;
    try {
      await runBusy({ title: "写入环境内的 pip 源", message: `正在写入 ${info.dir}\\pip.ini，只对这个环境生效。` },
        () => invoke("pip_apply_source", { scope: "custom", path: `${info.dir}\\pip.ini`, mirrorId: pipPick }));
      const next = await inspect(info.project);
      setDetail(next);
      toast("环境内的 pip 源已写入", "ok");
    } catch (e) {
      toast("写入环境内的 pip 源失败：" + e, "err");
    }
  }

  async function remove(info: VenvInfo) {
    setRemoving(null);
    try {
      await invoke("python_venv_remove", { dir: info.dir });
      await inspect(info.project);
      toast("虚拟环境已移到回收站", "ok");
    } catch (e) {
      toast("删除虚拟环境失败：" + e, "err");
    }
  }

  function copy(text: string, what: string) {
    navigator.clipboard.writeText(text).then(() => toast(`已复制${what}`, "ok"), () => toast("复制失败", "err"));
  }

  return (
    <>
      <div className="grouphd" style={{ marginTop: 18 }}>
        <span className="gt"><i className="ti ti-box" /> 项目环境 <span className="cnt">{projects.length} 个</span></span>
        <div className="ghr">
          <button className="gh xs" onClick={() => void scan()}><i className="ti ti-scan" /> 扫描本机</button>
          <button className="gh xs" onClick={() => void addProject()}><i className="ti ti-folder-plus" /> 添加项目</button>
        </div>
      </div>
      {projects.length === 0 && (
        <div className="banner gray"><i className="ti ti-info-circle lead" /><div className="bt">
          每个项目用自己的环境装依赖，互不影响，也不影响默认 Python。此处管理两类：项目里的虚拟环境（.venv），以及随项目分发、自带 pythonXY._pth 的嵌入式 Python。可以扫描本机找出来，也可以直接添加项目文件夹。
        </div></div>
      )}
      {projects.map((project) => {
        const info = infos[project];
        const broken = info?.dir && (!info.python || !info.baseExists);
        return (
          <div className="vrow" key={project}>
            <span className="ver" title={project}>{folderName(project)}</span>
            <span className="meta" title={info?.dir ?? project}>
              {!info ? "检测中…"
                : info.dir ? <>
                  {broken ? <span className="bd w">已损坏</span> : <span className="bd g">{kindName(info.kind)} {info.version ?? ""}</span>}
                  {" "}<code className="py-path">{info.python ?? info.dir}</code>
                </>
                : <><span className="bd n">没有虚拟环境</span> <code className="py-path">{project}</code></>}
            </span>
            <div className="acts">
              {info && !info.dir && (
                usable.length ? <>
                  <Select value={pick[project] ?? fallback} width={150}
                    onChange={(v) => setPick((cur) => ({ ...cur, [project]: v }))}
                    options={usable.map((v) => ({ value: v.version, label: v.version + (v.isDefault ? "（默认）" : "") }))} />
                  <button className="pr sm" onClick={() => void create(project)}><i className="ti ti-plus" /> 创建 .venv</button>
                </> : <span className="bd n">先安装 Python</span>
              )}
              {info?.dir && <button className="gh xs" title="路径、激活命令、pip 源和给 AI 的说明" onClick={() => { setDetail(info); setPipPick(""); }}><i className="ti ti-info-circle" /> 详情</button>}
              <button className="gh xs" title={`打开目录：${info?.dir ?? project}`} onClick={() => openFolder(info?.dir ?? project)}><i className="ti ti-folder-open" /></button>
              {info?.kind === "venv" && <button className="gh xs danger" title="把虚拟环境移到回收站" onClick={() => setRemoving(info)}><i className="ti ti-trash" /></button>}
              <button className="gh xs" title="从列表移除（不删除任何文件）" onClick={() => update(projects.filter((p) => p !== project))}><i className="ti ti-x" /></button>
            </div>
          </div>
        );
      })}

      {detail && (
        <Modal title={`${kindName(detail.kind)} · ${folderName(detail.project)}`} icon="ti-box" wide onClose={() => setDetail(null)}
          footer={<>
            <button className="gh sm" onClick={() => copy(venvSummary(detail), "给 AI 的说明")}><i className="ti ti-copy" /> 复制给 AI</button>
            <button className="pr sm" onClick={() => setDetail(null)}>关闭</button>
          </>}>
          <div className="py-venv-detail">
            {!detail.baseExists && <div className="banner amber"><i className="ti ti-alert-triangle lead" /><div className="bt">创建它的 Python 已不存在（{detail.base ?? "未知"}），这个虚拟环境不能用了。用下面的「更换 Python 版本」重建即可。</div></div>}
            {detail.kind === "embedded" && <div className="banner gray"><i className="ti ti-package lead" /><div className="bt">这是随项目分发的嵌入式 Python（目录里有 pythonXY._pth），由项目自己带着走。Stacker 只展示它，不会改动或删除。</div></div>}
            {[
              ["解释器", detail.python ?? "（缺少 Scripts\\python.exe）"],
              ["环境目录", detail.dir ?? ""],
              ...(detail.kind === "venv" ? [
                ["创建自", `${detail.base ?? "未知"}${detail.version ? `（Python ${detail.version}）` : ""}`],
                ["PowerShell 激活", `& "${detail.dir}\\Scripts\\Activate.ps1"`],
                ["cmd 激活", `"${detail.dir}\\Scripts\\activate.bat"`],
              ] : []),
              ["安装依赖", `"${detail.python}" -m pip install -r requirements.txt`],
            ].map(([label, value]) => (
              <div className="py-venv-line" key={label}>
                <span className="k">{label}</span>
                <code>{value}</code>
                <button className="gh xs" title="复制" onClick={() => copy(value, label)}><i className="ti ti-copy" /></button>
              </div>
            ))}
            <div className="py-venv-line">
              <span className="k">环境内 pip 源</span>
              <code title={detail.pipIni ?? undefined}>{detail.pipIni ?? "未设置（用当前用户的 pip.ini）"}</code>
              <Select value={pipPick || (pipMirrors[0]?.id ?? "")} width={150} onChange={setPipPick}
                options={pipMirrors.map((m) => ({ value: m.id, label: m.name }))} />
              <button className="gh xs" title={`写入 ${detail.dir}\\pip.ini，只对这个环境生效`} onClick={() => void applyPipMirror(detail)}>
                <i className="ti ti-check" /> 写入
              </button>
            </div>
            {detail.kind === "venv" && (
              <div className="py-venv-line">
                <span className="k">更换 Python</span>
                <code>重建这个虚拟环境，可选择保留已装的包</code>
                {usable.length ? <>
                  <Select value={rebuild?.version ?? fallback} width={150}
                    onChange={(v) => setRebuild({ info: detail, version: v, keep: true })}
                    options={usable.map((v) => ({ value: v.version, label: v.version + (v.isDefault ? "（默认）" : "") }))} />
                  <button className="gh xs" onClick={() => setRebuild({ info: detail, version: rebuild?.version ?? fallback, keep: true })}>
                    <i className="ti ti-refresh" /> 重建
                  </button>
                </> : <span className="bd n">先安装 Python</span>}
              </div>
            )}
            <div className="s dim">PowerShell 提示“禁止运行脚本”时，不用激活也可以：直接用上面的解释器绝对路径运行即可。</div>
          </div>
        </Modal>
      )}
      {rebuild && (
        <ConfirmModal title={`用 Python ${rebuild.version} 重建虚拟环境`} icon="ti-refresh" danger
          message={<>
            <div>虚拟环境不能原地换解释器。Stacker 会把 <code>{rebuild.info.dir}</code> 移到回收站，再用 Python {rebuild.version} 在同一位置重建。</div>
            <label className="ck" style={{ marginTop: 8 }}>
              <input type="checkbox" checked={rebuild.keep} onChange={(e) => setRebuild({ ...rebuild, keep: e.target.checked })} />
              先记录已装的包（pip freeze），重建后自动重新安装
            </label>
            <div>不勾选就是一个干净的新环境，依赖需要自己重装。重建期间请不要运行这个项目。</div>
          </>}
          confirmLabel="开始重建"
          onConfirm={() => void runRebuild()}
          onClose={() => setRebuild(null)} />
      )}
      {rebuilt && (
        <Modal title="重建结果" icon="ti-list-check" onClose={() => setRebuilt(null)}>
          <ul className="py-remove-list">
            {rebuilt.notes.map((note, i) => <li key={i} className={note.includes("失败") ? "bad" : "ok"}>
              <i className={"ti " + (note.includes("失败") ? "ti-alert-circle" : "ti-circle-check")} /> {note}
            </li>)}
          </ul>
          <div className="py-venv-line"><span className="k">新的解释器</span><code>{rebuilt.info.python ?? "缺失"}</code></div>
        </Modal>
      )}
      {removing && (
        <ConfirmModal title="删除虚拟环境" icon="ti-trash" danger
          message={<>
            <div>将把 <code>{removing.dir}</code> 移到回收站，可以从回收站还原。</div>
            <div>项目代码不受影响；里面装的依赖需要重新创建后再安装。</div>
          </>}
          confirmLabel="移到回收站"
          onConfirm={() => void remove(removing)}
          onClose={() => setRemoving(null)} />
      )}
    </>
  );
}
