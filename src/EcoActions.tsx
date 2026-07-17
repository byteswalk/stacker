import { useToast } from "./ui";
import { translateText } from "./i18n";

export type Shells = { powershell: boolean; gitbash: boolean; cmd: boolean };
export type EcosystemId = "python" | "node" | "java" | "maven" | "gradle" | "go" | "rust";

export function EcoActions({
  summary,
}: {
  ecosystem: EcosystemId;
  shells: Shells;
  summary: string;
}) {
  const toast = useToast();

  async function copySummary() {
    try {
      await navigator.clipboard.writeText(translateText(summary));
      toast("已复制摘要给 AI", "ok");
    } catch (e) {
      toast(`复制摘要给 AI 失败：${e}`, "err");
    }
  }

  return (
    <button className="gh sm" style={{ marginLeft: 8 }} title="复制当前生态摘要，方便 AI 判断可用工具、默认配置和操作边界" onClick={copySummary}>
      <i className="ti ti-copy" /> 复制摘要给 AI
    </button>
  );
}

export function summaryLine(label: string, value: unknown) {
  const text = value === null || value === undefined || value === "" ? "未检测到" : String(value);
  return `- ${label}：${text}`;
}
