import { App as AntApp, ConfigProvider, theme } from "antd";
import enUS from "antd/locale/en_US";
import zhCN from "antd/locale/zh_CN";
import dayjs from "dayjs";
import "dayjs/locale/zh-cn";
import { createContext, use, useEffect, useMemo, useState, type ReactNode } from "react";
import { englishUi, setLanguage } from "../i18n";
import { DEFAULT_PREFS, loadPrefs, resolveMode, savePrefs, type Prefs } from "./prefs";
import "./style.css";

/** Stacker's own palette, so the extension's pages read as the same product. */
const DARK = {
  colorPrimary: "#f5821f",
  colorInfo: "#6aa3f5",
  colorSuccess: "#6bcf86",
  colorWarning: "#e4b450",
  colorError: "#e2625b",
  colorBgLayout: "#13161c",
  colorBgContainer: "#1b2029",
  colorBgElevated: "#20262f",
  colorBorder: "rgba(255,255,255,.11)",
  colorBorderSecondary: "rgba(255,255,255,.06)",
  colorText: "#e7e9ec",
  colorTextSecondary: "#a8aeb9",
  colorTextTertiary: "#828995",
  colorTextQuaternary: "#5a616d",
  colorLink: "#6aa3f5",
};

const LIGHT = {
  colorPrimary: "#d9730d",
  colorInfo: "#2f6fd0",
  colorSuccess: "#2f9e57",
  colorWarning: "#b7791f",
  colorError: "#d03c36",
  colorBgLayout: "#f3f4f6",
  colorBgContainer: "#ffffff",
  colorBgElevated: "#ffffff",
  colorLink: "#2f6fd0",
};

const SHARED = {
  borderRadius: 8,
  fontSize: 13,
  controlHeight: 30,
  fontFamily: 'system-ui, "Microsoft YaHei", "PingFang SC", sans-serif',
  wireframe: false,
};

function components(dark: boolean) {
  return {
    Layout: {
      headerBg: dark ? "#10131a" : "#ffffff",
      headerHeight: 48,
      headerPadding: "0 16px",
      siderBg: dark ? "#10131a" : "#fafafa",
      bodyBg: dark ? "#13161c" : "#f3f4f6",
    },
    Menu: {
      itemHeight: 32,
      itemMarginInline: 6,
      itemSelectedBg: dark ? "rgba(245,130,31,.14)" : "rgba(217,115,13,.12)",
      itemSelectedColor: dark ? "#f5a45a" : "#b45e09",
    },
    Table: {
      headerBg: dark ? "#171c24" : "#fafafa",
      rowHoverBg: dark ? "#20262f" : "#f5f5f5",
      rowSelectedBg: dark ? "rgba(245,130,31,.10)" : "rgba(217,115,13,.08)",
      rowSelectedHoverBg: dark ? "rgba(245,130,31,.16)" : "rgba(217,115,13,.14)",
      cellPaddingBlock: 9,
    },
    Tag: { defaultBg: dark ? "rgba(255,255,255,.06)" : "#fafafa" },
    Card: { bodyPadding: 14, headerHeight: 40 },
  };
}

interface PrefsApi {
  prefs: Prefs;
  update: (patch: Partial<Prefs>) => void;
}

const PrefsContext = createContext<PrefsApi>({ prefs: DEFAULT_PREFS, update: () => {} });
export const usePrefs = () => use(PrefsContext);

/**
 * Theme, language and antd's message/modal context for both pages. Nothing renders until the
 * stored preference is in, so the page never flashes the wrong language or the wrong theme.
 */
export function Shell({ children }: { children: ReactNode }) {
  const [prefs, setPrefs] = useState<Prefs | null>(null);
  const [systemDark, setSystemDark] = useState(() => resolveMode("auto") === "dark");

  useEffect(() => { void loadPrefs().then(setPrefs); }, []);
  useEffect(() => {
    if (typeof matchMedia !== "function") return;
    const media = matchMedia("(prefers-color-scheme: light)");
    const follow = () => setSystemDark(!media.matches);
    media.addEventListener("change", follow);
    return () => media.removeEventListener("change", follow);
  }, []);

  const api = useMemo<PrefsApi>(() => ({
    prefs: prefs ?? DEFAULT_PREFS,
    update: (patch) => setPrefs((old) => {
      const next = { ...(old ?? DEFAULT_PREFS), ...patch };
      void savePrefs(next);
      return next;
    }),
  }), [prefs]);

  const dark = prefs && prefs.mode !== "auto" ? prefs.mode === "dark" : systemDark;
  useEffect(() => {
    document.documentElement.dataset.theme = dark ? "dark" : "light";
    document.documentElement.style.colorScheme = dark ? "dark" : "light";
  }, [dark]);

  if (!prefs) return null;
  setLanguage(prefs.lang);
  const english = englishUi();
  dayjs.locale(english ? "en" : "zh-cn");

  return <ConfigProvider
    locale={english ? enUS : zhCN}
    button={{ autoInsertSpace: false }}
    theme={{
      algorithm: dark ? theme.darkAlgorithm : theme.defaultAlgorithm,
      token: { ...SHARED, ...(dark ? DARK : LIGHT) },
      components: components(dark),
    }}
  >
    <AntApp className="shell">
      <PrefsContext value={api}>{children}</PrefsContext>
    </AntApp>
  </ConfigProvider>;
}
