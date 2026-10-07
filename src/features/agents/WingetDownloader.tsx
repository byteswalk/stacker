import { useEffect, useState } from "react";
import { invoke } from "../../invoke";
import { Select } from "../../Select";
import { useToast } from "../../ui";
import { useI18n } from "../../i18n";

type Downloader = { value: string; path: string; available: boolean; backedUp: boolean };

const ERRORS: Record<string, string> = {
  E_WINGET_SETTINGS: "WinGet 的设置文件格式不对，没有改动。可以用 winget settings 打开检查。",
  E_NO_WINGET: "本机没有找到 WinGet。",
};

const HINT: Record<string, string> = {
  default: "大安装包交给 Windows 的“传递优化”下载：没有进度，有时很慢。这是 WinGet 的默认做法。",
  wininet: "WinGet 自己下载，走系统代理，有进度。改的是 WinGet 自己的设置，命令行里用 winget 也一样。",
  do: "WinGet 设置里指定了只用“传递优化”下载。",
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
      const done = value === "wininet" ? "已改为 WinGet 自己下载。" : "已改回跟随系统。";
      toast(tr(done) + (next.backedUp ? tr("原设置文件已备份到“配置备份”。") : ""), "ok");
    } catch (e) {
      toast(ERRORS[String(e)] ?? String(e), "err");
    } finally {
      setBusy(false);
    }
  }

  const value = state?.value ?? "default";
  const options = [
    { value: "default", label: tr("跟随系统（传递优化）") },
    { value: "wininet", label: tr("WinGet 自己下载") },
    ...(value === "do" ? [{ value: "do", label: tr("只用传递优化") }] : []),
  ];
  return <div className="srcrow">
    <span className="av st"><i className="ti ti-download" /></span>
    <div className="mt">
      <div className="t">{tr("WinGet 下载方式")}</div>
      <div className="s dim" title={state?.path || undefined}>
        {state && !state.available ? tr("本机没有找到 WinGet。") : tr(HINT[value] ?? HINT.default)}
      </div>
    </div>
    <Select value={value} width={200} disabled={busy || !state?.available} onChange={(next) => void change(next)} options={options} />
  </div>;
}
