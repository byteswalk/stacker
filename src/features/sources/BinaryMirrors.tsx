import { useCallback, useEffect, useState } from "react";
import { invoke } from "../../invoke";
import { useToast, useBusy } from "../../ui";
import { Select } from "../../Select";

export type BinaryMirrorVar = { name: string; value: string; current: string | null; scope: string | null; matched: boolean };
export type BinaryMirrorState = {
  id: string;
  name: string;
  source_name: string;
  icon: string;
  description: string;
  enabled: boolean;
  configured: boolean;
  user_configured: boolean;
  system_configured: boolean;
  status_label: string;
  vars: BinaryMirrorVar[];
};
type HostPing = { host: string; ms: number | null };

const OFFICIAL_HOSTS: Record<string, string> = {
  electron: "github.com",
  browser: "playwright.azureedge.net",
  cypress: "download.cypress.io",
  native: "github.com",
  huggingface: "huggingface.co",
};

const AVATAR: Record<string, string> = { huggingface: "py" };

/**
 * Download mirrors that are environment variables rather than config files (Electron,
 * Playwright, Hugging Face …). Each ecosystem page shows the rows that belong to it; the
 * variables themselves are per user, so a row must appear on one page only.
 */
export function BinaryMirrors({ ids, note, onRows }: { ids: string[]; note?: string; onRows?: (rows: BinaryMirrorState[]) => void }) {
  const wanted = ids.join(",");
  const toast = useToast();
  const runBusy = useBusy();
  const [rows, setRows] = useState<BinaryMirrorState[]>([]);
  const [pending, setPending] = useState<Record<string, string>>({});
  const [pings, setPings] = useState<Record<string, number | null>>({});
  const [busy, setBusy] = useState("");

  const refresh = useCallback(() => {
    invoke<BinaryMirrorState[]>("binary_mirror_status").then((all) => {
      onRows?.(all);
      setRows(all.filter((row) => wanted.split(",").includes(row.id)));
      setPending((current) => {
        const next = { ...current };
        all.forEach((row) => {
          if (!(row.id in next)) next[row.id] = row.enabled ? "recommended" : row.configured ? "custom" : "official";
        });
        return next;
      });
    }).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wanted]);

  useEffect(refresh, [refresh]);

  async function apply(row: BinaryMirrorState) {
    const key = `binary:${row.id}`;
    const selected = pending[row.id] ?? (row.enabled ? "recommended" : "official");
    if (selected === "custom") return toast("当前为外部自定义配置，请选择官方默认或推荐镜像后再应用。", "info");
    setBusy(key);
    try {
      const useRecommended = selected === "recommended";
      const next = await runBusy({
        title: `应用 ${row.name} 下载源`,
        message: useRecommended
          ? "正在写入当前用户环境变量；新打开的终端和安装命令会读取推荐镜像。"
          : "正在清除当前用户镜像变量；新打开的终端将恢复工具官方默认地址。",
      }, () => invoke<BinaryMirrorState>(useRecommended ? "binary_mirror_apply" : "binary_mirror_clear", { id: row.id }));
      setRows((current) => current.map((item) => item.id === next.id ? next : item));
      setPending((current) => ({ ...current, [row.id]: useRecommended ? "recommended" : "official" }));
      toast(`${row.name} 已切换到${useRecommended ? `${next.source_name} 镜像` : "官方默认地址"}，新终端生效`, "ok");
    } catch (e) {
      toast(`启用 ${row.name} 下载镜像失败。请稍后重试。原因：` + e, "err");
    } finally {
      setBusy("");
    }
  }

  async function speedtest() {
    const hosts = [...new Set(rows.flatMap((row) => [
      OFFICIAL_HOSTS[row.id],
      row.vars[0]?.value ? new URL(row.vars[0].value).hostname : "",
    ]).filter(Boolean))];
    try {
      const result = await runBusy({
        title: "下载镜像测速",
        message: "正在比较各下载场景的官方地址与推荐镜像；单个地址 1500ms 无响应算超时。",
      }, () => invoke<HostPing[]>("speedtest_hosts", { hosts }));
      const pingMap: Record<string, number | null> = {};
      result.forEach((row) => { pingMap[row.host] = row.ms; });
      setPings(pingMap);
      setPending((current) => {
        const next = { ...current };
        rows.forEach((row) => {
          const officialHost = OFFICIAL_HOSTS[row.id];
          const mirrorHost = row.vars[0]?.value ? new URL(row.vars[0].value).hostname : "";
          const officialMs = pingMap[officialHost];
          const mirrorMs = pingMap[mirrorHost];
          if (typeof mirrorMs === "number" && (typeof officialMs !== "number" || mirrorMs < officialMs)) next[row.id] = "recommended";
          else if (typeof officialMs === "number") next[row.id] = "official";
        });
        return next;
      });
      toast("测速完成，已为各下载场景预选响应更快的地址；点击“应用”后生效。", "ok");
    } catch (e) {
      toast("下载镜像测速失败：" + e, "err");
    }
  }

  async function clear(row: BinaryMirrorState) {
    const key = `binary-clear:${row.id}`;
    setBusy(key);
    try {
      const next = await runBusy({
        title: `清除 ${row.name} 下载镜像`,
        message: "正在清除当前用户环境变量；新打开的终端会恢复默认下载地址。",
      }, () => invoke<BinaryMirrorState>("binary_mirror_clear", { id: row.id }));
      setRows((current) => current.map((item) => item.id === next.id ? next : item));
      if (next.system_configured) {
        toast(`${row.name} 的用户配置已清除，但系统级环境变量仍在生效`, "info");
      } else {
        toast(`${row.name} 下载镜像已清除，已恢复工具默认地址（新终端生效）`, "ok");
      }
    } catch (e) {
      toast(`清除 ${row.name} 下载镜像失败。请稍后重试。原因：` + e, "err");
    } finally {
      setBusy("");
    }
  }

  function badge(row: BinaryMirrorState) {
    if (row.enabled) return <span className="bd g">已配置</span>;
    if (row.configured) return <span className="bd w">自定义</span>;
    return <span className="bd n">默认</span>;
  }

  function hint(row: BinaryMirrorState) {
    return row.vars.map((v) => `${v.name} = ${v.current ?? "未设置"}${v.scope ? `（${v.scope}）` : ""}\n内置推荐：${v.value}`).join("\n\n");
  }

  function pingText(host: string) {
    if (typeof pings[host] === "number") return ` · ${pings[host]}ms`;
    return host in pings ? " · 超时" : "";
  }

  if (rows.length === 0) return null;
  return (
    <>
      <div className="srctoolbar">
        <div className="mt">
          <div className="s dim" title={note ?? "每个下载场景可独立使用工具官方默认地址或源目录中的推荐镜像。测速只负责预选，点击“应用”后才会修改当前用户环境变量。"}>
            {note ?? "各下载场景独立选源；测速后点击“应用”生效，新终端读取新配置。"}
          </div>
        </div>
        <button className="gh sm" disabled={!!busy} onClick={speedtest}><i className="ti ti-bolt" /> 测速</button>
      </div>
      {rows.map((row) => (
        <div className="srcrow" key={row.id}>
          <span className={"av " + (AVATAR[row.id] ?? "npm")}><i className={"ti " + row.icon} /></span>
          <div className="mt">
            <div className="t">{row.name} {badge(row)}</div>
            <div className="s dim" title={hint(row)}>{row.description}</div>
            <div className="s mono" title={hint(row)}>{row.vars[0]?.name}：{row.enabled ? `${row.source_name} 镜像` : row.configured ? `外部自定义配置 · ${row.vars[0]?.scope ?? "已生效"}` : "官方默认地址"}</div>
          </div>
          <Select value={pending[row.id] ?? (row.enabled ? "recommended" : row.configured ? "custom" : "official")} width={210}
            onChange={(value) => setPending((current) => ({ ...current, [row.id]: value }))}
            options={[
              { value: "official", label: `官方默认${pingText(OFFICIAL_HOSTS[row.id])}` },
              { value: "recommended", label: `${row.source_name} 镜像${pingText(row.vars[0]?.value ? new URL(row.vars[0].value).hostname : "")}` },
              ...(row.configured && !row.enabled ? [{ value: "custom", label: "外部自定义配置" }] : []),
            ]} />
          <button className="pr sm" disabled={!!busy} onClick={() => apply(row)}>
            <i className={"ti " + (busy === `binary:${row.id}` ? "ti-loader spin" : "ti-check")} /> 应用
          </button>
          <button className="gh sm" title={row.system_configured && !row.user_configured ? "当前仅有系统级配置，需以管理员权限在系统环境变量中清除" : "清除当前用户配置并恢复工具默认地址"} disabled={!!busy || !row.user_configured} onClick={() => clear(row)}>
            <i className={"ti " + (busy === `binary-clear:${row.id}` ? "ti-loader spin" : "ti-eraser")} /> 清除
          </button>
        </div>
      ))}
    </>
  );
}
