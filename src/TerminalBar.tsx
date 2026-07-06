import { type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useToast } from "./ui";

type Shells = { powershell: boolean; gitbash: boolean; cmd: boolean };

/** 统一的「终端集成」条。
 *  - 只显示「终端集成」四个字 + 一个 (?) 说明气泡（解释全进 tip，不占版面）。
 *  - avail：本机是否装了该终端；没装的（如未装 Git Bash）置灰、不可点，集成时也跳过。
 *  - 绿色可点：在 Stacker 所在目录直接开对应终端，便于验证。 */
export function TerminalBar({ avail, tip, action }: { avail: Shells; tip: string; action?: ReactNode }) {
  const toast = useToast();
  const open = async (kind: string) => {
    try { await invoke("open_shell", { kind }); } catch (e) { toast("打开失败：" + e, "err"); }
  };
  const items: [keyof Shells, string][] = [["powershell", "PowerShell"], ["gitbash", "Git Bash"], ["cmd", "cmd"]];
  return (
    <div className="banner gray" style={{ alignItems: "center" }}>
      <i className="ti ti-terminal-2 lead" />
      <div className="bt" style={{ display: "flex", alignItems: "center", gap: 6, flex: "0 0 auto" }}>
        <b>终端集成</b>
        <i className="ti ti-help-circle" title={tip} style={{ cursor: "help", opacity: 0.55, fontSize: 15 }} />
      </div>
      <div className="shells" style={{ marginLeft: "auto" }}>
        {items.map(([k, label]) => {
          const ok = avail[k];
          return (
            <span key={k} className={"shb " + (ok ? "ok" : "off")} style={ok ? { cursor: "pointer" } : undefined}
              title={ok ? `点击在 Stacker 所在目录打开 ${label} 验证` : `本机未安装 ${label}，已自动跳过`}
              onClick={ok ? () => open(k) : undefined}>
              <i className={"ti " + (ok ? "ti-terminal-2" : "ti-x")} /> {label}
              {ok && <i className="ti ti-external-link" style={{ fontSize: 11, marginLeft: 4, opacity: 0.7 }} />}
            </span>
          );
        })}
      </div>
      {action}
    </div>
  );
}
