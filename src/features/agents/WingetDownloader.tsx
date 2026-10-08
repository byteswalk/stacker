import { useEffect, useState } from "react";
import { invoke } from "../../invoke";
import { Select } from "../../Select";
import { useToast } from "../../ui";
import { useI18n } from "../../i18n";

type Downloader = { value: string; path: string; available: boolean; backedUp: boolean };

const ERRORS: Record<string, string> = {
  E_WINGET_SETTINGS: "WinGet 的设置文件格式无效，未做修改。可运行 winget settings 打开检查。",
  E_NO_WINGET: "本机没有找到 WinGet。",
};

const HINT: Record<string, string> = {
  default: "WinGet 默认设置：大安装包交给 Windows 传递优化服务下载，任务中不显示下载进度，部分网络下速度较慢。",
  wininet: "使用 WinINet 下载：经系统代理，任务中显示下载进度。此项写入 WinGet 的 network.downloader 设置，命令行中的 winget 同样生效。",
  do: "WinGet 设置中 network.downloader 已指定为 do，所有下载都使用传递优化。",
};

/** WinGet's own download setting: left as Windows has it, or WinGet downloading by itself. */
export function WingetDownloader() {
  const toast = useToast();
  const { tr } = useI18n();
  const [state, setState] = useState<Downloader | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { void invoke<Downloader>("winget_downloader").then(setState).catch(() => setState(null)); }, []);

  async function change(value: string) {
    setBusy(true);
    try {
      const next = await invoke<Downloader>("winget_downloader_set", { value });
      setState(next);
      const done = value === "wininet" ? "WinGet 下载器已设为 WinINet。" : "WinGet 下载器已恢复默认。";
      toast(tr(done) + (next.backedUp ? tr("原设置文件已备份到“配置备份”。") : ""), "ok");
    } catch (e) {
      toast(ERRORS[String(e)] ?? String(e), "err");
    } finally {
      setBusy(false);
    }
  }

  const value = state?.value ?? "default";
  const options = [
    { value: "default", label: tr("默认（传递优化）") },
    { value: "wininet", label: tr("WinINet") },
    ...(value === "do" ? [{ value: "do", label: tr("传递优化（强制）") }] : []),
  ];
  return <div className="srcrow">
    <span className="av st"><i className="ti ti-download" /></span>
    <div className="mt">
      <div className="t">{tr("WinGet 下载器")}</div>
      <div className="s dim" title={state?.path || undefined}>
        {state && !state.available ? tr("本机没有找到 WinGet。") : tr(HINT[value] ?? HINT.default)}
      </div>
    </div>
    <Select value={value} width={200} disabled={busy || !state?.available} onChange={(next) => void change(next)} options={options} />
  </div>;
}
