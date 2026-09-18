import { ConversationManager } from "../features/conversations/ConversationManager";
import type { Page } from "../pageState";

export default function AgentSpace({ goto }: { goto: (page: Page) => void }) {
  return <ConversationManager onCleanup={() => goto("cleanup")} />;
}
