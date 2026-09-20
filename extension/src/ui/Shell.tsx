import { App as AntApp, ConfigProvider, theme } from "antd";
import enUS from "antd/locale/en_US";
import zhCN from "antd/locale/zh_CN";
import dayjs from "dayjs";
import "dayjs/locale/zh-cn";
import { createContext, use, useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { englishUi, setLanguage } from "../i18n";
import { bridgeStatus, callStacker, type BridgeStatus } from "../lib/bridgeMessages";
import {
  cleanPrefs, DEFAULT_PREFS, loadPrefs, modeOfTheme, PREFS_KEY, resolveMode, savePrefs, themeOfMode, type Prefs,
} from "./prefs";
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
  /** Stacker's connection, polled here because the appearance rides along with it. */
  bridge: BridgeStatus | null;
  reconnect: () => void;
}

const PrefsContext = createContext<PrefsApi>({
  prefs: DEFAULT_PREFS, update: () => {}, bridge: null, reconnect: () => {},
});
export const usePrefs = () => use(PrefsContext);

const POLL_MS = 5000;

/**
 * Theme, language and antd's message/modal context for both pages. Nothing renders until the
 * stored preference is in, so the page never flashes the wrong language or the wrong theme.
 */
export function Shell({ children }: { children: ReactNode }) {
  const [prefs, setPrefs] = useState<Prefs | null>(null);
  const [systemDark, setSystemDark] = useState(() => resolveMode("auto") === "dark");
  const [bridge, setBridge] = useState<BridgeStatus | null>(null);
  /** The appearance sent to Stacker and not yet seen coming back. */
  const [awaited, setAwaited] = useState<string | null>(null);

  useEffect(() => { void loadPrefs().then(setPrefs); }, []);
  useEffect(() => {
    if (typeof matchMedia !== "function") return;
    const media = matchMedia("(prefers-color-scheme: light)");
    const follow = () => setSystemDark(!media.matches);
    media.addEventListener("change", follow);
    return () => media.removeEventListener("change", follow);
  }, []);

  // The manage page and the popup are separate windows; a change in one reaches the other.
  useEffect(() => {
    if (typeof chrome === "undefined" || !chrome.storage?.onChanged) return;
    const follow = (changes: Record<string, chrome.storage.StorageChange>, area: string) => {
      if (area === "local" && changes[PREFS_KEY]) setPrefs(cleanPrefs(changes[PREFS_KEY].newValue));
    };
    chrome.storage.onChanged.addListener(follow);
    return () => chrome.storage.onChanged.removeListener(follow);
  }, []);

  // Stacker and the extension show the same appearance whenever they are bridged.
  const poll = useCallback(async (force = false) => {
    try {
      setBridge(await bridgeStatus(true, force));
    } catch {
      setBridge(null);
    }
  }, []);

  useEffect(() => {
    let alive = true;
    const tick = () => { if (alive) void poll(); };
    tick();
    const timer = setInterval(tick, POLL_MS);
    return () => { alive = false; clearInterval(timer); };
  }, [poll]);

  // Adopt Stacker's appearance, except while waiting for it to confirm one sent from here:
  // a status already in flight when it was sent would otherwise undo the user's pick.
  useEffect(() => {
    if (!prefs || !bridge?.connected) {
      if (awaited !== null) setAwaited(null);
      return;
    }
    if (awaited !== null) {
      if (bridge.theme === awaited) setAwaited(null);
      return;
    }
    const mode = modeOfTheme(bridge.theme);
    if (!mode || mode === prefs.mode) return;
    const next = { ...prefs, mode };
    setPrefs(next);
    void savePrefs(next);
  }, [bridge, prefs, awaited]);

  const api = useMemo<PrefsApi>(() => ({
    prefs: prefs ?? DEFAULT_PREFS,
    bridge,
    reconnect: () => void poll(true),
    update: (patch) => {
      const base = prefs ?? DEFAULT_PREFS;
      const next = { ...base, ...patch };
      setPrefs(next);
      void savePrefs(next);
      if (next.mode !== base.mode) {
        const shared = themeOfMode(next.mode);
        setAwaited(shared);
        // Not connected, or Stacker refused: the choice stays local and Stacker's wins again later.
        void callStacker("setTheme", { theme: shared }).catch(() => setAwaited(null));
      }
    },
  }), [prefs, bridge, poll]);

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
