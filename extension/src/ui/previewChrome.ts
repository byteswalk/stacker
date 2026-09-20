/** Preview only: the few chrome.* calls the pages make, backed by plain maps. Imported first. */
export const session = new Map<string, unknown>();
export const local = new Map<string, unknown>();
/** Stands in for Stacker's own appearance so the shared-theme path can be tried out. */
export const stacker = { theme: "system" };
// Exposed so the appearance handshake can be driven from the browser console:
//   stackerPreview.theme = "dark"   // the pages follow within a few seconds
Object.assign(globalThis, { stackerPreview: stacker });

const area = (store: Map<string, unknown>) => ({
  get: async (key: string) => (store.has(key) ? { [key]: store.get(key) } : {}),
  set: async (values: Record<string, unknown>) => { for (const [k, v] of Object.entries(values)) store.set(k, v); },
  onChanged: { addListener: () => {}, removeListener: () => {} },
});

Object.assign(globalThis, {
  chrome: {
    storage: { local: area(local), session: area(session) },
    runtime: {
      getURL: (path: string) => path,
      sendMessage: async (m: { type: string; call?: string; payload?: { theme?: string } }) => {
        if (m.type === "bridge-status") {
          return { ok: true, value: { connected: true, pending: 2, lastSyncAt: Date.now(), error: "", theme: stacker.theme } };
        }
        if (m.type === "bridge-call" && m.call === "setTheme" && m.payload?.theme) {
          stacker.theme = m.payload.theme;
          return { ok: true, value: { theme: stacker.theme } };
        }
        return { ok: true, value: null };
      },
    },
    tabs: { create: () => {} },
  },
});
