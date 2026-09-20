/** Preview only: the few chrome.* calls the pages make, backed by plain maps. Imported first. */
export const session = new Map<string, unknown>();
export const local = new Map<string, unknown>();

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
      sendMessage: async (m: { type: string }) =>
        (m.type === "bridge-status"
          ? { ok: true, value: { connected: true, pending: 2, lastSyncAt: Date.now(), error: "" } }
          : { ok: true, value: null }),
    },
    tabs: { create: () => {} },
  },
});
