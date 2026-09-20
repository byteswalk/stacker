/** Appearance and language of the extension's own pages, kept in the browser. */
export type Lang = "auto" | "zh" | "en";
export type Mode = "auto" | "dark" | "light";
export interface Prefs {
  lang: Lang;
  mode: Mode;
}

const KEY = "stacker-web-chats-prefs";
/** Stacker's own window is dark by default; its pages match. */
export const DEFAULT_PREFS: Prefs = { lang: "auto", mode: "dark" };

const LANGS: Lang[] = ["auto", "zh", "en"];
const MODES: Mode[] = ["auto", "dark", "light"];

function clean(value: unknown): Prefs {
  const v = value as Partial<Prefs> | null;
  return {
    lang: LANGS.includes(v?.lang as Lang) ? (v!.lang as Lang) : DEFAULT_PREFS.lang,
    mode: MODES.includes(v?.mode as Mode) ? (v!.mode as Mode) : DEFAULT_PREFS.mode,
  };
}

/** chrome.storage in the extension, localStorage in tests and the design preview. */
export async function loadPrefs(): Promise<Prefs> {
  try {
    if (typeof chrome !== "undefined" && chrome.storage?.local) return clean((await chrome.storage.local.get(KEY))[KEY]);
    const text = localStorage.getItem(KEY);
    return clean(text ? JSON.parse(text) : null);
  } catch {
    return { ...DEFAULT_PREFS };
  }
}

export async function savePrefs(prefs: Prefs): Promise<void> {
  try {
    if (typeof chrome !== "undefined" && chrome.storage?.local) await chrome.storage.local.set({ [KEY]: prefs });
    else localStorage.setItem(KEY, JSON.stringify(prefs));
  } catch {
    /* the pages keep working with the prefs held in memory */
  }
}

/** The dark or light actually shown, with "auto" resolved against the system. */
export function resolveMode(mode: Mode): "dark" | "light" {
  if (mode !== "auto") return mode;
  return typeof matchMedia === "function" && matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}
