import { useEffect, useState } from "react";
import { invoke } from "../../invoke";
import { formatBytes } from "../../notifications";
import { useI18n } from "../../i18n";
import { useToast } from "../../ui";

type StoragePlace = { key: string; path: string; exists: boolean; bytes: number };

const PLACES: Record<string, { icon: string; title: string; detail: string }> = {
  program: { icon: "ti-app-window", title: "程序", detail: "Stacker 程序本身。" },
  data: { icon: "ti-settings", title: "设置与资料", detail: "设置、配置备份、密钥库、Git 账号、源配置和空间分析快照。" },
  runtimes: { icon: "ti-stack-2", title: "运行时", detail: "Stacker 下载安装的 JDK、Go、Maven、Gradle 等。" },
  tools: { icon: "ti-tool", title: "版本管理工具", detail: "fnm、pyenv 以及 pyenv 装的 Python 版本。" },
  conversations: { icon: "ti-messages", title: "会话与网页对话", detail: "会话收藏与摘要、网页对话同步和导出文件。" },
  logs: { icon: "ti-file-text", title: "日志", detail: "运行日志，按天归档。" },
};

/** Where Stacker keeps its program, data and what it downloaded and installed. */
export function StoragePlaces() {
  const { tr } = useI18n();
  const toast = useToast();
  const [places, setPlaces] = useState<StoragePlace[] | null>(null);

  useEffect(() => {
    let alive = true;
    invoke<StoragePlace[]>("storage_places").then((list) => { if (alive) setPlaces(list); }).catch(() => { if (alive) setPlaces([]); });
    return () => { alive = false; };
  }, []);

  const open = (key: string) => {
    invoke("storage_place_open", { key }).catch((error) => toast(tr("打开文件夹失败：") + error, "err"));
  };

  if (places === null) return <div className="srcrow"><div className="mt"><div className="s dim">{tr("正在统计存储位置…")}</div></div></div>;
  return <>
    {places.map((place) => {
      const meta = PLACES[place.key];
      if (!meta) return null;
      return <div className="srcrow storage-place" key={place.key}>
        <span className="av st"><i className={"ti " + meta.icon} /></span>
        <div className="mt">
          <div className="t">{tr(meta.title)} {place.exists && <span className="bd n">{formatBytes(place.bytes)}</span>}</div>
          <div className="s dim">{tr(meta.detail)}</div>
          <div className="s mono" title={place.path}>{place.path}</div>
        </div>
        <button className="gh sm" disabled={!place.exists} title={place.exists ? undefined : tr("这个文件夹还没有创建")} onClick={() => open(place.key)}>
          <i className="ti ti-folder-open" /> {place.exists ? tr("打开") : tr("尚未创建")}
        </button>
      </div>;
    })}
    <div className="callout"><i className="ti ti-info-circle" /><div>{tr("智能体、桌面应用、Node 版本（fnm）和 Rust 工具链由各自的安装器装到它们的默认位置，不在上面这些文件夹里。")}</div></div>
  </>;
}
