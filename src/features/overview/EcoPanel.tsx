import type { CSSProperties } from "react";
import { useI18n } from "../../i18n";
import type { Page } from "../../pageState";

export type EcosystemSnapshot = {
  id: Page;
  label: string;
  kind: string;
  status: "ok" | "warn" | "missing" | "unknown";
  summary: string;
  detail: string;
};
export type EcoTone = "ok" | "warn" | "idle" | "unknown";

const TONE_TEXT: Record<EcoTone, string> = { ok: "正常", warn: "需处理", idle: "未配置", unknown: "未知" };
// A source Stacker cannot name says nothing worth a place on the line.
const UNNAMED_SOURCES = new Set(["—", "未识别"]);

const ECO_META: Record<string, { av: string; icon: string; label: string; hue: string }> = {
  git: { av: "st", icon: "ti-brand-git", label: "Git", hue: "#aeb6c0" },
  python: { av: "py", icon: "ti-brand-python", label: "Python", hue: "#7cc1ff" },
  php: { av: "php", icon: "ti-brand-php", label: "PHP", hue: "#aeb9ff" },
  node: { av: "npm", icon: "ti-brand-nodejs", label: "Node.js", hue: "#f08aa0" },
  java: { av: "jv", icon: "ti-coffee", label: "Java", hue: "#9fb3d6" },
  go: { av: "go", icon: "ti-brand-golang", label: "Go", hue: "#67d4e6" },
  maven: { av: "mv2", icon: "ti-feather", label: "Maven", hue: "#8fa8e0" },
  gradle: { av: "gr", icon: "ti-box", label: "Gradle", hue: "#7fd1c6" },
  rust: { av: "rs", icon: "ti-brand-rust", label: "Rust", hue: "#e89b6c" },
};

/** The version in a tool's own words: "git version 2.56.0.windows.3", "pyenv 3.1.1 · Python 3.13.16". */
export function ecoVersion(summary: string) {
  const last = summary.split("·").pop() ?? summary;
  return last.match(/\d+(?:\.\d+)+/)?.[0] ?? summary;
}

/** Who manages the versions, when the tool's summary names one: "pyenv 3.1.1 · Python 3.13.16". */
export function ecoManager(summary: string) {
  const parts = summary.split("·").map((part) => part.trim());
  // Name and version: rustup goes on with a build hash and date.
  return parts.length > 1 ? parts[0].split(/\s+/).slice(0, 2).join(" ") : null;
}

/** The source worth naming: one Stacker cannot identify says nothing. */
export function ecoSource(source: string) {
  return UNNAMED_SOURCES.has(source) ? null : source;
}

export function ecoTone(item: Pick<EcosystemSnapshot, "status">, hasIssue: boolean): EcoTone {
  if (hasIssue || item.status === "warn") return "warn";
  if (item.status === "unknown") return "unknown";
  return item.status === "missing" ? "idle" : "ok";
}

export function EcoTally({ tones, updates }: { tones: EcoTone[]; updates: number }) {
  const { tr: t } = useI18n();
  const count = (tone: EcoTone) => tones.filter((item) => item === tone).length;
  return <span className="eco-tally">
    <span className="eco-health" aria-hidden="true">{tones.map((tone, index) => <i key={index} className={tone} />)}</span>
    <b className="ok">{count("ok")}</b> {t("正常")} · <b className="warn">{count("warn")}</b> {t("需处理")} · {count("idle")} {t("未配置")}
    {updates > 0 && <> · <b className="update">{updates}</b> {t("可更新")}</>}
  </span>;
}

/** Every ecosystem at a glance: one panel, three to a row, each row opening its page. */
export function EcoPanel({ items, toneOf, sourceOf, latestOf, onOpen }: {
  items: EcosystemSnapshot[];
  toneOf: (item: EcosystemSnapshot) => EcoTone;
  sourceOf: (item: EcosystemSnapshot) => string;
  latestOf: (item: EcosystemSnapshot) => string | undefined;
  onOpen: (page: Page) => void;
}) {
  const { tr: t } = useI18n();
  return <div className="eco-panel">
    {items.map((item) => {
      const meta = ECO_META[item.id];
      const tone = toneOf(item);
      const known = item.status !== "missing" && item.status !== "unknown";
      const manager = ecoManager(item.summary);
      const source = known ? ecoSource(sourceOf(item)) : null;
      const latest = latestOf(item);
      return <button type="button" className={"eco-cell " + tone} key={item.id} title={item.detail || item.summary}
        style={{ "--eco": meta.hue } as CSSProperties} onClick={() => onOpen(item.id)}>
        <i className={"ti eco-mark " + meta.icon} aria-hidden="true" />
        <span className={"av " + meta.av}><i className={"ti " + meta.icon} aria-hidden="true" /></span>
        <span className="eco-text">
          <span className="eco-name"><b>{meta.label}</b>{manager && <small>{manager}</small>}</span>
          {known
            ? <span className="eco-line">
              <code>{ecoVersion(item.summary)}</code>
              {source && <span className="eco-source"><i className="ti ti-cloud-download" aria-hidden="true" />{source}</span>}
            </span>
            : <span className="eco-line dim">{item.summary}</span>}
        </span>
        {latest
          ? <em className="update" title={t("有新版本 {version}").replace("{version}", latest)}><i className="ti ti-arrow-up" aria-hidden="true" />{latest}</em>
          : <em className={tone}><i aria-hidden="true" />{t(TONE_TEXT[tone])}</em>}
      </button>;
    })}
  </div>;
}
