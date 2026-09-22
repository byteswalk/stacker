import { describe, expect, it, vi } from "vitest";
import type { VibeTool } from "../features/agents/catalogStore";

vi.mock("../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn(), reportFrontendError: vi.fn() }));
const { sortTools } = await import("./Agents");

const tool = (id: string, name: string, sort_order: number) => ({ id, name, sort_order }) as unknown as VibeTool;
const tools = [
  tool("qoder", "Qoder 国际版", 131),
  tool("claude", "Claude Code", 20),
  tool("qoder-cn", "Qoder 中国版", 130),
  tool("codex", "Codex", 10),
  tool("mimo-cn", "小米 MiMo 中国版", 150),
];
const ids = (list: VibeTool[]) => list.map((t) => t.id);

describe("agent list order", () => {
  it("defaults to popularity, the China edition before the global one", () => {
    expect(ids(sortTools(tools, "default"))).toEqual(["codex", "claude", "qoder-cn", "qoder", "mimo-cn"]);
  });

  it("sorts by name either way", () => {
    expect(ids(sortTools(tools, "az")).slice(0, 2)).toEqual(["claude", "codex"]);
    expect(ids(sortTools(tools, "za"))).toEqual([...ids(sortTools(tools, "az"))].reverse());
  });
});
