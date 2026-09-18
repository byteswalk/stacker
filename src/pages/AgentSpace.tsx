import { useEffect, useState } from "react";
import { ManagedWorkSession } from "../features/agent-workspace/ManagedWorkSession";
import { ConversationManager } from "../features/conversations/ConversationManager";
import { useI18n } from "../i18n";
import type { Page } from "../pageState";
import {
  runVibeCheck,
  subscribeVibe,
  surfaceDetected,
  vibeSnapshot,
  type VibeTool,
} from "./Vibe";

export default function AgentSpace({ goto }: { goto: (page: Page) => void }) {
  const { t } = useI18n();
  const initial = vibeSnapshot();
  const [tools, setTools] = useState<VibeTool[]>(initial.tools);
  const [checked, setChecked] = useState(initial.checked);
  const [loading, setLoading] = useState(initial.loading);

  useEffect(() => subscribeVibe((next) => {
    setTools(next.tools);
    setChecked(next.checked);
    setLoading(next.loading);
  }), []);

  const agents = tools.map((tool) => ({
    id: tool.id,
    name: tool.name,
    cliInstalled: surfaceDetected(tool.cli),
    cliPath: tool.cli.path ?? null,
    desktopInstalled: surfaceDetected(tool.desktop),
    desktopPath: tool.desktop.path ?? null,
    desktopName: tool.desktop.label,
  }));

  const copy = {
    title: t("nav.agentSpace"), subtitle: t("agentSpace.subtitle"),
    refresh: checked ? t("agentSpace.refresh") : t("agentSpace.readStatus"), refreshing: t("agentSpace.reading"),
    guardTitle: t("agentSpace.guardTitle"), guardDescription: t("agentSpace.guardDescription"),
    trackTitle: t("agentSpace.trackTitle"), trackDescription: t("agentSpace.trackDescription"),
    reviewTitle: t("agentSpace.reviewTitle"), reviewDescription: t("agentSpace.reviewDescription"),
    workspaceLabel: t("nav.agentSpace"),
  };

  return <ConversationManager onCleanup={() => goto("cleanup")} advanced={<>
      <button className="gh sm" disabled={loading} onClick={() => void runVibeCheck().catch(() => undefined)}>
        <i className={`ti ${loading ? "ti-loader spin" : "ti-refresh"}`} />
        {loading ? copy.refreshing : copy.refresh}
      </button>
    <ManagedWorkSession agents={agents} onOpenCleanup={() => goto("cleanup")} />
  </>} />;
}
