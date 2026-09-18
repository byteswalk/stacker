import { describe, expect, it } from "vitest";
import { aiToolUpdatesFrom } from "./notifications";

const surface = (update: boolean, label = "CLI") => ({ label, version: "1.0", latest: update ? "2.0" : "1.0", update_available: update });

describe("agent update notices", () => {
  it("follow the catalog and count a shared CLI once", () => {
    const tools = [
      { id: "workbuddy-cn", name: "WorkBuddy 中国版", cli_id: "codebuddy", cli: surface(true), desktop: surface(false, "桌面端") },
      { id: "workbuddy-global", name: "WorkBuddy 国际版", cli_id: "codebuddy", cli: surface(true), desktop: surface(false, "桌面端") },
      { id: "kimi", name: "Kimi", cli_id: "kimi", cli: surface(false), desktop: surface(true, "Kimi Work 桌面端") },
    ];
    const notices = aiToolUpdatesFrom(tools);
    expect(notices.map((item) => item.id)).toEqual(["workbuddy-cn:CLI", "kimi:Kimi Work 桌面端"]);
    expect(aiToolUpdatesFrom([{ ...tools[2], desktop: surface(false, "Kimi Work 桌面端") }])).toEqual([]);
  });
});
