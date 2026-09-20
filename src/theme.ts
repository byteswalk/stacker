// 外观主题：dark / light / system。data-theme 写在 <html> 上（CSS 据此切换变量）。
// 持久化到 localStorage（即时、无闪烁），后端 settings.json 也存一份（换机/导出一致）。
export type Theme = "dark" | "light" | "system";

const KEY = "stacker-theme";
const mql = () => window.matchMedia("(prefers-color-scheme: light)");

export function getTheme(): Theme {
  const t = localStorage.getItem(KEY);
  return t === "light" || t === "system" ? t : "dark";
}

function resolve(t: Theme): "dark" | "light" {
  return t === "system" ? (mql().matches ? "light" : "dark") : t;
}

export function applyTheme(t: Theme = getTheme()) {
  document.documentElement.setAttribute("data-theme", resolve(t));
}

export function setTheme(t: Theme) {
  localStorage.setItem(KEY, t);
  applyTheme(t);
}

// 跟随系统时，系统明暗变化要实时反映
export function watchSystemTheme() {
  mql().addEventListener("change", () => {
    if (getTheme() === "system") applyTheme("system");
  });
}

/** 外观被本窗口之外改过（目前只有桥接的浏览器插件）时派发，供界面刷新选择框。 */
export const THEME_CHANGED_EVENT = "stacker-theme-changed";

function isTheme(value: unknown): value is Theme {
  return value === "dark" || value === "light" || value === "system";
}

/**
 * 浏览器插件桥接后也能改外观（写的是同一份 settings.json）。
 * 这里定期核对一次，把外部改动应用到本窗口；窗口不可见时不查。
 */
export function watchSharedTheme(read: () => Promise<unknown>, intervalMs = 4000) {
  const check = async () => {
    if (document.visibilityState !== "visible") return;
    try {
      const remote = await read();
      if (!isTheme(remote) || remote === getTheme()) return;
      localStorage.setItem(KEY, remote);
      applyTheme(remote);
      window.dispatchEvent(new CustomEvent(THEME_CHANGED_EVENT));
    } catch {
      // 后端暂时不可用：保持当前外观，下次再核对。
    }
  };
  const onFocus = () => void check();
  const timer = setInterval(onFocus, intervalMs);
  window.addEventListener("focus", onFocus);
  return () => {
    clearInterval(timer);
    window.removeEventListener("focus", onFocus);
  };
}
