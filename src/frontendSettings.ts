export type FrontendSettings = Record<string, string>;

/**
 * The page preferences that belong to an environment: where each ecosystem downloads from,
 * which releases it offers, and the tool files it is pointed at. Everything else Stacker keeps
 * in the browser store (column widths, AI answers, scan history, cached states) is this
 * computer's own and never travels with a profile.
 */
const PREFERENCES = new Set([
  "stacker.java.vendor",
  "stacker.maven.customSettingsXml",
  "stacker.gradle.customInitGradle",
  "stacker.gradle.wrapperPath",
  "stacker.gradle.wrapperSource",
  "stacker.pip.customPath",
]);

export function isPreference(key: string): boolean {
  return PREFERENCES.has(key) || /^stacker\.[\w-]+\.(downloadSource|install\.\w+)$/.test(key);
}

export function collectFrontendSettings(storage: Storage = localStorage): FrontendSettings {
  const settings: FrontendSettings = {};
  for (let index = 0; index < storage.length; index += 1) {
    const key = storage.key(index);
    if (!key || !isPreference(key)) continue;
    const value = storage.getItem(key);
    if (value !== null) settings[key] = value;
  }
  return settings;
}

/**
 * Puts a profile's preferences in place. A preference the profile does not have goes back to
 * its default; nothing that is not a preference is touched, whatever an older profile carried.
 */
export function restoreFrontendSettings(
  settings: FrontendSettings,
  storage: Storage = localStorage,
) {
  // 旧版配置没有该字段；空对象保持当前偏好，避免导入旧文件时重置界面。
  if (Object.keys(settings).length === 0) return;
  const stale: string[] = [];
  for (let index = 0; index < storage.length; index += 1) {
    const key = storage.key(index);
    if (key && isPreference(key) && !(key in settings)) stale.push(key);
  }
  stale.forEach((key) => storage.removeItem(key));
  Object.entries(settings).forEach(([key, value]) => {
    if (isPreference(key)) storage.setItem(key, value);
  });
}
