import { SessionCatalog } from "../features/sessions/SessionCatalog";
import type { Page } from "../pageState";

export default function AgentData({ goto }: { goto: (page: Page) => void }) {
  return <SessionCatalog onCleanup={() => goto("cleanup")} />;
}
