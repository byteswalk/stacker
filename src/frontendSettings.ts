export type FrontendSettings = Record<string, string>;

export function collectFrontendSettings(storage: Storage = localStorage): FrontendSettings {
  const settings: FrontendSettings = {};
  for (let index = 0; index < storage.length; index += 1) {
    const key = storage.key(index);
    if (!key?.startsWith("stacker.")) continue;
    const value = storage.getItem(key);
    if (value !== null) settings[key] = value;
  }
  return settings;
}

export function restoreFrontendSettings(
  settings: FrontendSettings,
  storage: Storage = localStorage,
) {
  // 旧版配置没有该字段；空对象保持当前偏好，避免导入旧文件时重置界面。
  if (Object.keys(settings).length === 0) return;
  const staleKeys: string[] = [];
  for (let index = 0; index < storage.length; index += 1) {
    const key = storage.key(index);
    if (key?.startsWith("stacker.") && !(key in settings)) staleKeys.push(key);
  }
  staleKeys.forEach((key) => storage.removeItem(key));
  Object.entries(settings).forEach(([key, value]) => {
    if (key.startsWith("stacker.")) storage.setItem(key, value);
  });
}
