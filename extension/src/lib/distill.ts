import { callStacker } from "./bridgeMessages";

/** 一条提炼结果（只读；提炼在 Stacker 里进行）。 */
export interface DistillItem {
  id: string;
  kind: string;
  title: string;
  body: string;
  state: string;
  updatedAt: number;
  sources: string[];
}

export const DISTILL_KIND_LABEL: Record<string, string> = {
  qa: "经验问答",
  requirement: "领域要求",
  prompt: "提示词",
  skill: "skill 草稿",
};

type Send = Parameters<typeof callStacker>[2];

/** 某条对话的提炼结果；Stacker 没连上或没结果时是空数组。 */
export async function distillResults(site: string, id: string, send?: Send): Promise<DistillItem[]> {
  const reply = (await callStacker("distillResults", { site, id }, send)) as { items?: unknown } | null;
  return Array.isArray(reply?.items) ? (reply.items as DistillItem[]) : [];
}
