import { useI18n } from "../../i18n";
import type { Page } from "../../pageState";

export type EcosystemSnapshot = {
  id: Page;
  label: string;
  kind: string;
  status: "ok" | "warn" | "missing";
  summary: string;
  detail: string;
};
export type EcoTone = "ok" | "warn" | "idle";

const TONE_TEXT: Record<EcoTone, string> = { ok: "正常", warn: "需处理", idle: "未配置" };
// A source Stacker cannot name says nothing worth a place on the line.
const UNNAMED_SOURCES = new Set(["—", "未识别"]);

const ECO_META: Record<string, { av: string; icon: string; label: string }> = {
  git: { av: "st", icon: "ti-brand-git", label: "Git" },
  python: { av: "py", icon: "ti-brand-python", label: "Python" },
  php: { av: "php", icon: "ti-brand-php", label: "PHP" },
  node: { av: "npm", icon: "ti-brand-nodejs", label: "Node.js" },
  java: { av: "jv", icon: "ti-coffee", label: "Java" },
  go: { av: "go", icon: "ti-brand-golang", label: "Go" },
  maven: { av: "mv2", icon: "ti-feather", label: "Maven" },
  gradle: { av: "gr", icon: "ti-box", label: "Gradle" },
  rust: { av: "rs", icon: "ti-brand-rust", label: "Rust" },
};

/** The version in a tool's own words: "git version 2.56.0.windows.3", "pyenv 3.1.1 · Python 3.13.16". */
export function ecoVersion(summary: string) {
  const last = summary.split("·").pop() ?? summary;
  return last.match(/\d+(?:\.\d+)+/)?.[0] ?? summary;
}

/** The line under an ecosystem's name: its version and where it downloads from. */
export function ecoLine(item: Pick<EcosystemSnapshot, "status" | "summary">, source: string) {
  if (item.status === "missing") return item.summary;
  return [ecoVersion(item.summary), ...(UNNAMED_SOURCES.has(source) ? [] : [source])].join(" · ");
}

export function ecoTone(item: Pick<EcosystemSnapshot, "status">, hasIssue: boolean): EcoTone {
  if (hasIssue || item.status === "warn") return "warn";
  return item.status === "missing" ? "idle" : "ok";
}

export function EcoTally({ tones }: { tones: EcoTone[] }) {
  const { tr: t } = useI18n();
  const count = (tone: EcoTone) => tones.filter((item) => item === tone).length;
  return <span className="eco-tally">
    <b className="ok">{count("ok")}</b> {t("正常")} · <b className="warn">{count("warn")}</b> {t("需处理")} · {count("idle")} {t("未配置")}
  </span>;
}

/** Every ecosystem at a glance: one panel, three to a row, each row opening its page. */
export function EcoPanel({ items, toneOf, sourceOf, onOpen }: {
  items: EcosystemSnapshot[];
  toneOf: (item: EcosystemSnapshot) => EcoTone;
  sourceOf: (item: EcosystemSnapshot) => string;
  onOpen: (page: Page) => void;
}) {
  const { tr: t } = useI18n();
  return <div className="eco-panel">
    {items.map((item) => {
      const meta = ECO_META[item.id];
      const tone = toneOf(item);
      return <button type="button" className="eco-cell" key={item.id} title={item.detail || item.summary} onClick={() => onOpen(item.id)}>
        <span className={"av " + meta.av}><i className={"ti " + meta.icon} aria-hidden="true" /></span>
        <span className="eco-text">
          <b>{meta.label}</b>
          <small>{ecoLine(item, sourceOf(item))}</small>
        </span>
        <em className={tone}>{t(TONE_TEXT[tone])}</em>
      </button>;
    })}
  </div>;
}
