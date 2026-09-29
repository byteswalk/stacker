import {
  getCurrentWindow,
  LogicalSize,
  PhysicalSize,
  type Window as AppWindow,
} from "@tauri-apps/api/window";

export const DEFAULT_WINDOW_SIZE = { width: 1280, height: 720 } as const;
export const MINIMUM_WINDOW_SIZE = { width: 1280, height: 720 } as const;

const WINDOW_SIZE_STORAGE_KEY = "stackerLocal.mainWindowSize.v1";
const RESIZE_SAVE_DELAY_MS = 250;

export type SavedWindowSize = {
  width: number;
  height: number;
};

export function normalizeClientSize(
  size: { width: number; height: number },
  scaleFactor: number,
): SavedWindowSize | null {
  if (!Number.isFinite(scaleFactor) || scaleFactor <= 0) return null;
  return normalizeWindowSize({
    width: size.width / scaleFactor,
    height: size.height / scaleFactor,
  });
}

export function normalizeWindowSize(value: unknown): SavedWindowSize | null {
  if (!value || typeof value !== "object") return null;
  const candidate = value as Partial<SavedWindowSize>;
  if (!Number.isFinite(candidate.width) || !Number.isFinite(candidate.height)) return null;

  return {
    width: Math.max(MINIMUM_WINDOW_SIZE.width, Math.round(candidate.width as number)),
    height: Math.max(MINIMUM_WINDOW_SIZE.height, Math.round(candidate.height as number)),
  };
}

function readSavedWindowSize(): SavedWindowSize | null {
  try {
    const raw = localStorage.getItem(WINDOW_SIZE_STORAGE_KEY);
    return raw ? normalizeWindowSize(JSON.parse(raw)) : null;
  } catch {
    return null;
  }
}

function saveWindowSize(size: SavedWindowSize): void {
  try {
    localStorage.setItem(WINDOW_SIZE_STORAGE_KEY, JSON.stringify(size));
  } catch {
    // Window preferences are non-critical; the configured default remains usable.
  }
}

/** The smallest the window may be, in the display's own pixels. A minimum given in logical
 *  pixels rounds down on a scaled display, which is how a window ends up a hair under it. */
async function applyMinimumSize(appWindow: AppWindow): Promise<void> {
  const scaleFactor = await appWindow.scaleFactor();
  await appWindow.setMinSize(new PhysicalSize(
    Math.ceil(MINIMUM_WINDOW_SIZE.width * scaleFactor),
    Math.ceil(MINIMUM_WINDOW_SIZE.height * scaleFactor),
  ));
}

export async function initializeMainWindowSize(
  appWindow: AppWindow = getCurrentWindow(),
): Promise<void> {
  await applyMinimumSize(appWindow);
  const saved = readSavedWindowSize();
  if (saved) {
    await appWindow.setSize(new LogicalSize(saved.width, saved.height));
    await appWindow.center();
  } else {
    saveWindowSize(DEFAULT_WINDOW_SIZE);
  }

  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  await appWindow.onResized(({ payload }) => {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      void (async () => {
        if (await appWindow.isMaximized() || await appWindow.isMinimized()) return;
        if (payload.width <= 0 || payload.height <= 0) return;
        const scaleFactor = await appWindow.scaleFactor();
        // Read the current client area instead of persisting the event snapshot.
        const inner = await appWindow.innerSize();
        const normalized = normalizeClientSize(inner, scaleFactor);
        if (!normalized) return;
        // A drag that got under the floor is put back on it, and never saved.
        if (inner.width / scaleFactor < MINIMUM_WINDOW_SIZE.width - 0.5
          || inner.height / scaleFactor < MINIMUM_WINDOW_SIZE.height - 0.5) {
          await applyMinimumSize(appWindow);
          await appWindow.setSize(new LogicalSize(normalized.width, normalized.height));
          return;
        }
        saveWindowSize(normalized);
      })();
    }, RESIZE_SAVE_DELAY_MS);
  });
}

export async function resetMainWindowSize(
  appWindow: AppWindow = getCurrentWindow(),
): Promise<void> {
  if (await appWindow.isMaximized()) {
    await appWindow.unmaximize();
  }
  await appWindow.setSize(
    new LogicalSize(DEFAULT_WINDOW_SIZE.width, DEFAULT_WINDOW_SIZE.height),
  );
  await appWindow.center();
  saveWindowSize(DEFAULT_WINDOW_SIZE);
}
