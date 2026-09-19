/**
 * What an adapter may read from its site's page besides making requests: one cookie or one localStorage entry.
 * Never page globals (a content script cannot see them) and never anything written back.
 */
export interface PageAccess {
  cookie(name: string): string | null;
  storage(key: string): string | null;
}

export const NO_PAGE: PageAccess = { cookie: () => null, storage: () => null };

export function pageOf(doc: { cookie: string }, getStorage: () => Pick<Storage, "getItem">): PageAccess {
  return {
    cookie(name) {
      for (const part of doc.cookie.split(";")) {
        const eq = part.indexOf("=");
        if (eq < 0 || part.slice(0, eq).trim() !== name) continue;
        const raw = part.slice(eq + 1).trim();
        try { return decodeURIComponent(raw); } catch { return raw; }
      }
      return null;
    },
    storage(key) {
      try { return getStorage().getItem(key); } catch { return null; }
    },
  };
}
