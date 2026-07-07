import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useToast, Modal, ConfirmModal, useBusy, Loading } from "../ui";
import { SourcesPanel } from "../SourcesPanel";

type Toolchain = { name: string; is_default: boolean };
type RustupStatus = { installed: boolean; rustup_version: string | null; toolchains: Toolchain[]; default: string | null };

export default function Rust() {
  const toast = useToast();
  const runBusy = useBusy();
  const [ru, setRu] = useState<RustupStatus | null>(null);
  const [loadErr, setLoadErr] = useState(false);
  const [busy, setBusy] = useState("");
  const [installOpen, setInstallOpen] = useState(false);
  const [channel, setChannel] = useState("stable");
  const [uninstall, setUninstall] = useState<string | null>(null);
  const [srcKey, setSrcKey] = useState(0); // 装完 rustup 后刷新 Cargo 换源面板的检测

  async function load() { setRu(await invoke<RustupStatus>("rustup_status")); }
  useEffect(() => { load().catch(() => setLoadErr(true)); }, []);

  async function act(fn: () => Promise<unknown>, ok: string, key: string): Promise<boolean> {
    setBusy(key);
    try { await fn(); await load(); toast(ok, "ok"); return true; }
    catch (e) { toast("失败：" + e, "err"); return false; } finally { setBusy(""); }
  }
  async function installRustup() {
    try {
      await runBusy({ title: "安装 rustup", message: "正在下载 rustup-init 并安装 stable 工具链，体积较大请耐心等待。", progressEvent: "install-progress", cancel: { label: "取消", onCancel: () => { invoke("op_cancel").catch(() => {}); } } }, () => invoke("rustup_install_self"));
      await load(); setSrcKey((k) => k + 1); toast("rustup 已安装，新终端生效", "ok");
    } catch (e) { toast("安装失败：" + e, "err"); }
  }
  async function updateRustup() {
    try {
      await runBusy({ title: "更新 rustup", message: "正在检查并更新 rustup。", progressEvent: "install-progress", cancel: { label: "取消", onCancel: () => { invoke("op_cancel").catch(() => {}); } } }, () => invoke("rustup_self_update"));
      await load(); toast("rustup 已是最新或已更新", "ok");
    } catch (e) { toast("更新失败：" + e, "err"); }
  }
  async function installChannel() {
    const c = channel.trim();
    setInstallOpen(false);
    try {
      await runBusy({ title: `安装工具链 ${c}`, message: "正在通过 rustup 下载并安装工具链。", progressEvent: "install-progress", cancel: { label: "取消安装", onCancel: () => { invoke("op_cancel").catch(() => {}); } } }, () => invoke("rustup_install", { channel: c }));
      await load(); toast("已安装 " + c, "ok");
    } catch (e) { toast("安装失败：" + e, "err"); }
  }

  if (loadErr) return <div className="stub"><div className="si"><i className="ti ti-plug-x" /></div><h2>读取 rustup 状态失败</h2><p>请在 Tauri 应用内运行（浏览器预览没有后端）。</p></div>;
  if (!ru) return <Loading text="正在检测 rustup 工具链…" />;

  return (
    <>
      <div className="grouphd">
        <span className="gt"><i className="ti ti-stack-2" /> 工具链 <span className="cnt">{ru.installed ? `rustup · ${ru.toolchains.length} 个` : "rustup"}</span></span>
        {ru.installed && <div className="ghr"><button className="gh xs" onClick={async () => { await load(); toast("已刷新", "ok"); }}><i className="ti ti-refresh" /> 刷新</button><button className="gh xs" title="rustup self update（检查并更新 rustup 自身）" onClick={updateRustup}><i className="ti ti-cloud-download" /> rustup 更新</button><button className="pr sm" onClick={() => setInstallOpen(true)}><i className="ti ti-plus" /> 安装工具链</button></div>}
      </div>
      {!ru.installed ? (
        <div className="banner blue" style={{ flexDirection: "column", alignItems: "stretch", gap: 9 }}>
          <div style={{ display: "flex", gap: 11, alignItems: "flex-start" }}>
            <i className="ti ti-download lead" />
            <div className="bt"><b>用 rustup 管理 Rust 工具链</b><br />Stacker 会安装 rustup，并预设 <span className="code">RUSTUP_DIST_SERVER</span>，用于后续工具链下载。Cargo 镜像可在下方单独配置。</div>
          </div>
          <div style={{ paddingLeft: 29 }}>
            <button className="pr sm" onClick={installRustup}><i className="ti ti-download" /> 一键安装 rustup</button>
          </div>
        </div>
      ) : ru.toolchains.length === 0 ? (
        <div className="banner gray"><i className="ti ti-info-circle lead" /><div className="bt">还没装任何工具链，点「安装工具链」装一个。</div></div>
      ) : ru.toolchains.map((t) => (
        <div className={"vrow" + (t.is_default ? " cur" : "")} key={t.name}>
          <span className="ver">{t.name}</span>
          <span className="meta">{t.is_default ? "默认工具链（rustup default）" : "已安装"}</span>
          <div className="acts">
            {t.is_default
              ? <><span className="live"><i className="ti ti-circle-check" /> 默认</span>
                  <button className="gh xs" disabled={!!busy} title="重新写入 rustup default 并刷新"
                    onClick={() => act(() => invoke("rustup_set_default", { name: t.name }), "已重新应用默认", "d" + t.name)}><i className="ti ti-refresh" /> 重新应用</button></>
              : <button className="pr sm" disabled={!!busy} onClick={() => act(() => invoke("rustup_set_default", { name: t.name }), "已设为默认 " + t.name, "d" + t.name)}>设为默认</button>}
            <button className="spd" disabled={!!busy} title="卸载" onClick={() => setUninstall(t.name)}><i className="ti ti-trash" /></button>
          </div>
        </div>
      ))}
      <div className="callout"><i className="ti ti-info-circle" /><div>组件（rustfmt / clippy）与交叉编译 target 的增删请在终端用 <span className="code">rustup component / target</span>；本页聚焦工具链与镜像配置。</div></div>

      <div className="grouphd" style={{ marginTop: 18 }}><span className="gt"><i className="ti ti-package" /> Cargo 源 <span className="cnt">~/.cargo/config.toml</span></span><span className="hint2">写入当前用户配置，改动前自动备份</span></div>
      <SourcesPanel toolIds={["cargo"]} refresh={srcKey} />

      {installOpen && (
        <Modal title="安装工具链" icon="ti-plus" onClose={() => setInstallOpen(false)}
          footer={<>
            <button className="gh sm" onClick={() => setInstallOpen(false)}>取消</button>
            <button className="pr sm" disabled={!channel.trim()} onClick={installChannel}>安装</button>
          </>}>
          <div className="field"><label>渠道 / 版本</label>
            <div className="seg" style={{ alignSelf: "flex-start" }}>
              {["stable", "beta", "nightly"].map((c) => <button key={c} className={channel === c ? "on" : ""} onClick={() => setChannel(c)}>{c}</button>)}
            </div>
            <input className="ip full" value={channel} onChange={(e) => setChannel(e.target.value)} placeholder="或输入具体版本，如 1.75.0" style={{ marginTop: 8 }} />
          </div>
          <div className="banner gray" style={{ margin: 0 }}><i className="ti ti-info-circle lead" /><div className="bt">下载走 rustup 配置的镜像（RUSTUP_DIST_SERVER）。</div></div>
        </Modal>
      )}

      {uninstall && (
        <ConfirmModal title={"卸载工具链 " + uninstall} icon="ti-trash" danger
          message={<>将运行 <span className="code">rustup toolchain uninstall {uninstall}</span>，删除该工具链及其组件 / target。不可撤销。</>}
          confirmLabel={busy === "u" + uninstall ? "卸载中…" : "确认卸载"} busy={busy === "u" + uninstall}
          onConfirm={async () => { if (await act(() => invoke("rustup_uninstall", { name: uninstall }), "已卸载 " + uninstall, "u" + uninstall)) setUninstall(null); }}
          onClose={() => setUninstall(null)} />
      )}
    </>
  );
}
