import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "../../invoke";
import { ConfirmModal, Modal, useBusy, useToast } from "../../ui";
import { Select } from "../../Select";
import { venvSummary, type VenvInfo } from "./venvSummary";

const PROJECTS_KEY = "stacker.python.venvProjects";

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
    // The list is a convenience; the venvs themselves live in the projects.
  }
}

type Version = { version: string; path?: string | null; isDefault: boolean };

/** Project virtual environments: find or create `.venv` from an installed version. */
export function VenvPanel({ versions, openFolder }: { versions: Version[]; openFolder: (path: string) => void }) {
  const toast = useToast();
  const runBusy = useBusy();
  const [projects, setProjects] = useState<string[]>(loadProjects);
  const [infos, setInfos] = useState<Record<string, VenvInfo>>({});
  const [pick, setPick] = useState<Record<string, string>>({});
  const [detail, setDetail] = useState<VenvInfo | null>(null);
  const [removing, setRemoving] = useState<VenvInfo | null>(null);
  const usable = versions.filter((v) => v.path);
  const fallback = (usable.find((v) => v.isDefault) ?? usable[0])?.version ?? "";

  async function inspect(project: string) {
    const info = await invoke<VenvInfo>("python_venv_inspect", { project });
    setInfos((cur) => ({ ...cur, [project]: info }));
    return info;
  }

  useEffect(() => {
    projects.forEach((p) => void inspect(p).catch(() => {}));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function update(list: string[]) {
    setProjects(list);
    saveProjects(list);
  }

  async function addProject() {
    const dir = await open({ directory: true, multiple: false, title: "选择项目文件夹" });
    if (!dir || typeof dir !== "string") return;
    if (!projects.some((p) => p.toLowerCase() === dir.toLowerCase())) update([dir, ...projects]);
    const info = await inspect(dir);
    toast(info.dir ? `已找到虚拟环境：${info.dir}` : "这个项目还没有虚拟环境，可以选版本创建", info.dir ? "ok" : "info");
  }

  async function create(project: string) {
    const version = usable.find((v) => v.version === (pick[project] ?? fallback));
    if (!version?.path) {
      toast("请先在上方安装一个 Python 版本", "info");
      return;
    }
    const python = `${version.path.replace(/[\\/]+$/, "")}\\python.exe`;
    try {
      const info = await runBusy(
        { title: "创建虚拟环境", message: `正在用 Python ${version.version} 在 ${project} 下创建 .venv…` },
        () => invoke<VenvInfo>("python_venv_create", { project, python }),
      );
      setInfos((cur) => ({ ...cur, [project]: info }));
      toast(`已创建 .venv（Python ${version.version}）`, "ok");
    } catch (e) {
      toast("创建虚拟环境失败：" + e, "err");
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
        <span className="gt"><i className="ti ti-box" /> 项目虚拟环境 <span className="cnt">{projects.length} 个项目</span></span>
        <div className="ghr">
          <button className="gh xs" onClick={() => void addProject()}><i className="ti ti-folder-plus" /> 添加项目</button>
        </div>
      </div>
      {projects.length === 0 && (
        <div className="banner gray"><i className="ti ti-info-circle lead" /><div className="bt">
          每个项目用自己的虚拟环境（.venv）装依赖，互不影响，也不会弄乱默认 Python。添加项目文件夹后，可以查看已有的虚拟环境，或用已安装的版本一键创建。
        </div></div>
      )}
      {projects.map((project) => {
        const info = infos[project];
        const broken = info?.dir && (!info.python || !info.baseExists);
        return (
          <div className="vrow" key={project}>
            <span className="ver" title={project}>{project.split(/[\\/]/).filter(Boolean).pop()}</span>
            <span className="meta" title={info?.dir ?? project}>
              {!info ? "检测中…"
                : info.dir ? <>{broken ? <span className="bd w">已损坏</span> : <span className="bd g">Python {info.version ?? "?"}</span>} <code className="py-path">{info.python ?? info.dir}</code></>
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
              {info?.dir && <button className="gh xs" title="路径、激活命令和给 AI 的说明" onClick={() => setDetail(info)}><i className="ti ti-info-circle" /> 详情</button>}
              <button className="gh xs" title={`打开项目目录：${project}`} onClick={() => openFolder(project)}><i className="ti ti-folder-open" /></button>
              {info?.dir && <button className="gh xs danger" title="把虚拟环境移到回收站" onClick={() => setRemoving(info)}><i className="ti ti-trash" /></button>}
              <button className="gh xs" title="从列表移除（不删除任何文件）" onClick={() => update(projects.filter((p) => p !== project))}><i className="ti ti-x" /></button>
            </div>
          </div>
        );
      })}

      {detail && (
        <Modal title={`虚拟环境 · ${detail.project.split(/[\\/]/).filter(Boolean).pop()}`} icon="ti-box" wide onClose={() => setDetail(null)}
          footer={<>
            <button className="gh sm" onClick={() => copy(venvSummary(detail), "给 AI 的说明")}><i className="ti ti-copy" /> 复制给 AI</button>
            <button className="pr sm" onClick={() => setDetail(null)}>关闭</button>
          </>}>
          <div className="py-venv-detail">
            {!detail.baseExists && <div className="banner amber"><i className="ti ti-alert-triangle lead" /><div className="bt">创建它的 Python 已不存在（{detail.base ?? "未知"}），这个虚拟环境不能用了。删除后用已安装的版本重新创建，再重新安装依赖。</div></div>}
            {[
              ["解释器", detail.python ?? "（缺少 Scripts\\python.exe）"],
              ["虚拟环境目录", detail.dir ?? ""],
              ["创建自", `${detail.base ?? "未知"}${detail.version ? `（Python ${detail.version}）` : ""}`],
              ["PowerShell 激活", `& "${detail.dir}\\Scripts\\Activate.ps1"`],
              ["cmd 激活", `"${detail.dir}\\Scripts\\activate.bat"`],
              ["安装依赖", `"${detail.python}" -m pip install -r requirements.txt`],
            ].map(([label, value]) => (
              <div className="py-venv-line" key={label}>
                <span className="k">{label}</span>
                <code>{value}</code>
                <button className="gh xs" title="复制" onClick={() => copy(value, label)}><i className="ti ti-copy" /></button>
              </div>
            ))}
            <div className="s dim">PowerShell 提示“禁止运行脚本”时，不用激活也可以：直接用上面的解释器绝对路径运行即可。</div>
          </div>
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
