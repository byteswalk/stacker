import { useState } from "react";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import type { Page } from "../../pageState";
import { AiAskModal, AiButton, askAi } from "./AiAsk";

/** The toolchain pages the header offers an explanation on. */
export const TOOLCHAIN_PAGES = new Set<Page>(["git", "python", "php", "node", "java", "maven", "gradle", "go", "rust"]);

type EcosystemItem = { id: string; label: string; status: string; summary: string; detail: string };
type EcosystemCheck = { ecosystems: EcosystemItem[] };
type CheckItem = { id: string; sev: string; title: string; desc: string; page: string };

/**
 * One button in the page header for every toolchain page: what this page's toolchain looks
 * like right now — what was detected, and anything the checkup flags — and what that means.
 */
export function ToolchainAsk({ page, label }: { page: Page; label: string }) {
  const { tr } = useI18n();
  const [open, setOpen] = useState(false);
  return <>
    <AiButton label={tr("AI 解释当前状态")} title={tr("把这一页检测到的版本和问题交给 AI，说清冲突在哪、实际用的是哪个")}
      onClick={() => setOpen(true)} />
    {open && <AiAskModal title={`${label} · ${tr("当前状态")}`} sub={tr("检测结果由 Stacker 判断，AI 只负责解释")}
      note={tr("只发送这一页的检测结果（版本、路径、状态），不读取你的项目文件。")}
      run={async () => {
        const [check, issues] = await Promise.all([
          invoke<EcosystemCheck>("coding_ecosystem_check").catch(() => ({ ecosystems: [] })),
          invoke<CheckItem[]>("checkup_page", { page }).catch(() => []),
        ]);
        return askAi("toolchain", {
          page: label,
          items: {
            detected: check.ecosystems.filter((item) => item.id === page),
            issues: issues.map((item) => ({ title: item.title, severity: item.sev, detail: item.desc })),
          },
        });
      }}
      onClose={() => setOpen(false)} />}
  </>;
}
