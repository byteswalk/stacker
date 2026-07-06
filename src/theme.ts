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
