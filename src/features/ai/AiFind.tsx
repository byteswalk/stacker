import { useState } from "react";
import { invoke } from "../../invoke";
import { useI18n } from "../../i18n";
import { useToast } from "../../ui";
import { aiError } from "./AiSettings";
import { AiSetupPrompt, needsAiSetup } from "./AiAsk";

/** One filter a list offers the AI: its name, what it means, and what it may hold. */
export type FilterField =
  | { name: string; meaning: string; kind: "text" | "number" | "bool" }
  | { name: string; meaning: string; kind: "choice"; options: string[] };

/**
 * "AI 查找": the sentence typed in the list's search box becomes the list's own filters, which
 * stay on screen where they can be read and undone. Only the sentence and the filter choices
 * go to the AI, never the rows.
 */
export function AiFindButton({ list, fields, query, onFilter, hint }: {
  /** The list's name, as the AI is told it. */
  list: string;
  fields: FilterField[];
  query: string;
  onFilter: (filter: Record<string, string | number | boolean>) => void;
  /** What to type, shown when the box is empty. */
  hint: string;
}) {
  const { tr } = useI18n();
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const [setup, setSetup] = useState<unknown>(null);
  async function find() {
    const words = query.trim();
    if (!words) { toast(tr(hint), "info"); return; }
    setBusy(true);
    try {
      onFilter(await invoke<Record<string, string | number | boolean>>("ai_list_filter", {
        list, query: words,
        fields: fields.map((field) => ({ ...field, options: field.kind === "choice" ? field.options : [] })),
      }));
    } catch (error) {
      if (needsAiSetup(error)) setSetup(error); else toast(tr(aiError(error)), "err");
    } finally { setBusy(false); }
  }
  return <>
    <button type="button" className="gh sm ai-btn" disabled={busy}
      title={tr("把搜索框里的一句话交给 AI，换成这里的筛选条件；只发送这句话和可选的筛选项")} onClick={() => void find()}>
      <i className={"ti " + (busy ? "ti-loader spin" : "ti-sparkles")} /> {tr("AI 查找")}
    </button>
    {setup !== null && <AiSetupPrompt error={setup} onClose={() => setSetup(null)} />}
  </>;
}
