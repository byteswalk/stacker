import { App as AntApp, Input, Tag } from "antd";
import { t } from "../i18n";
import type { SiteId } from "../shared/types";
import { SITES } from "../sites/registry";

export type ModalApi = ReturnType<typeof AntApp.useApp>["modal"];

/** Stacker's mark, so both windows are recognisably the same product. */
export function Brand({ text = "Stacker 网页对话" }: { text?: string }) {
  return <span className="brand">
    <span className="logo" aria-hidden="true">
      <svg viewBox="0 0 32 32" focusable="false">
        <path fill="#ff8422" d="M16 4 28 10 16 16 4 10Z" />
        <path fill="#33d2c7" d="M16 10 28 16 16 22 4 16Z" />
        <path fill="#6383ff" d="M16 16 28 22 16 28 4 22Z" />
      </svg>
    </span>
    <span>{t(text)}</span>
  </span>;
}

const SITE_COLOR: Record<SiteId, string> = {
  chatgpt: "#10a37f",
  claude: "#c96442",
  gemini: "#4285f4",
  grok: "#6b7280",
  deepseek: "#4d6bfe",
};

export function SiteTag({ site }: { site: SiteId }) {
  return <Tag color={SITE_COLOR[site]} style={{ marginInlineEnd: 0 }}>{SITES[site].label}</Tag>;
}

/**
 * The one-field dialog that replaces window.prompt: a folder name, a tag, an account alias.
 * Resolves with the trimmed text, or null when the user backs out or leaves it empty.
 */
export function askText(modal: ModalApi, opts: { title: string; placeholder?: string; value?: string }): Promise<string | null> {
  return new Promise((resolve) => {
    let text = opts.value ?? "";
    let settled = false;
    let handle: { destroy: () => void } | null = null;
    const done = (value: string | null) => {
      if (settled) return;
      settled = true;
      resolve(value);
    };
    const submit = () => {
      const value = text.trim();
      done(value || null);
    };
    handle = modal.confirm({
      title: opts.title,
      icon: null,
      okText: t("确定"),
      cancelText: t("取消"),
      content: <Input
        autoFocus
        defaultValue={opts.value}
        placeholder={opts.placeholder}
        maxLength={80}
        onChange={(e) => { text = e.target.value; }}
        onPressEnter={() => { handle?.destroy(); submit(); }}
      />,
      onOk: submit,
      onCancel: () => done(null),
    });
  });
}
