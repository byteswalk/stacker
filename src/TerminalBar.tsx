import { type ReactNode, useState } from "react";
import { invoke } from "./invoke";
import { translateText } from "./i18n";
import { Modal, useToast } from "./ui";

type Shells = { powershell: boolean; gitbash: boolean; cmd: boolean };
type EcosystemId = "python" | "node" | "java" | "maven" | "gradle" | "go" | "rust" | "git";
type ActivationCommands = { powershell: string; gitbash: string; cmd: string };

export function TerminalBar({
  avail,
  tip,
  action,
  ecosystem,
  summary,
}: {
  avail: Shells;
  tip: string;
  action?: ReactNode;
  ecosystem?: EcosystemId;
  summary?: string;
}) {
  const toast = useToast();
  const [activation, setActivation] = useState<ActivationCommands | null>(null);
  const [activationShell, setActivationShell] = useState<keyof ActivationCommands>("powershell");
  const open = async (kind: string) => {
    try {
      if (ecosystem) await invoke("open_ecosystem_verify_shell", { kind, ecosystem });
      else await invoke("open_shell", { kind, cwd: null, command: null });
    } catch (e) {
      toast("打开终端失败：" + e, "err");
    }
  };
  const copySummary = async () => {
    if (!summary) return;
    try {
      await navigator.clipboard.writeText(translateText(summary));
      toast("已复制摘要给 AI", "ok");
    } catch (e) {
      toast("复制摘要给 AI 失败：" + e, "err");
    }
  };
  const showActivation = async () => {
    if (!ecosystem) return;
    try {
      setActivation(await invoke<ActivationCommands>("ecosystem_activation_commands", { ecosystem }));
    } catch (e) {
      toast("生成临时生效命令失败：" + e, "err");
    }
  };
  const copyActivation = async () => {
    if (!activation) return;
    try {
      await navigator.clipboard.writeText(activation[activationShell]);
      toast(`已复制 ${activationShell === "powershell" ? "PowerShell" : activationShell === "gitbash" ? "Git Bash" : "cmd"} 临时生效命令`, "ok");
    } catch (e) {
      toast("复制临时生效命令失败：" + e, "err");
    }
  };
  const items: [keyof Shells, string][] = [["powershell", "PowerShell"], ["gitbash", "Git Bash"], ["cmd", "cmd"]];
  return (
    <div className="banner gray terminal-bar" style={{ alignItems: "center" }}>
      <i className="ti ti-terminal-2 lead" />
      <div className="bt" style={{ display: "flex", alignItems: "center", gap: 6, flex: "0 0 auto" }}>
        <b>终端集成</b>
        <i className="ti ti-help-circle" title={tip} style={{ cursor: "help", opacity: 0.55, fontSize: 15 }} />
      </div>
      <div className="shells" style={{ marginLeft: "auto" }}>
        {items.map(([k, label]) => {
          const ok = avail[k];
          return (
            <span
              key={k}
              className={"shb " + (ok ? "ok" : "off")}
              style={ok ? { cursor: "pointer" } : undefined}
              title={ok ? (ecosystem ? `打开 ${label} 并自动验证当前生态环境` : `打开 ${label}`) : `本机未检测到 ${label}`}
              onClick={ok ? () => open(k) : undefined}
            >
              <i className={"ti " + (ok ? "ti-terminal-2" : "ti-x")} /> {label}
              {ok && <i className="ti ti-external-link" style={{ fontSize: 11, marginLeft: 4, opacity: 0.7 }} />}
            </span>
          );
        })}
      </div>
      {ecosystem && (
        <button className="gh sm" style={{ marginLeft: 8 }} title="复制命令到已打开的终端执行，让该终端立即读取当前环境配置" onClick={showActivation}>
          <i className="ti ti-bolt" /> 临时生效
        </button>
      )}
      {action}
      {summary && (
        <button className="gh sm" style={{ marginLeft: 8 }} title="复制当前生态摘要，方便 AI 判断可用工具、默认配置和操作边界" onClick={copySummary}>
          <i className="ti ti-copy" /> 复制摘要给 AI
        </button>
      )}
      {activation && (
        <Modal title="让已打开的终端临时生效" icon="ti-bolt" onClose={() => setActivation(null)}
          footer={<>
            <button className="gh sm" onClick={() => setActivation(null)}>关闭</button>
            <button className="pr sm" onClick={copyActivation}><i className="ti ti-copy" /> 复制命令</button>
          </>}>
          <div className="seg" style={{ alignSelf: "flex-start", marginBottom: 10 }}>
            {(["powershell", "gitbash", "cmd"] as const).map((kind) => (
              <button key={kind} className={activationShell === kind ? "on" : ""} onClick={() => setActivationShell(kind)}>
                {kind === "powershell" ? "PowerShell" : kind === "gitbash" ? "Git Bash" : "cmd"}
              </button>
            ))}
          </div>
          <div className="banner gray" style={{ margin: "0 0 10px" }}><i className="ti ti-info-circle lead" /><div className="bt">在目标终端中执行下方命令，只更新当前终端会话；关闭终端后自动失效，不会修改系统配置。</div></div>
          <textarea className="promptbox" style={{ minHeight: 150 }} readOnly value={activation[activationShell]} />
        </Modal>
      )}
    </div>
  );
}
