import type { ListPage, RemoteAccount, RemoteBody, SiteId } from "../shared/types";
import type { FetchJson } from "./http";
import type { PageAccess } from "./page";

export interface Adapter {
  site: SiteId;
  origin: string;
  account(): Promise<RemoteAccount>;
  list(cursor: string | null): Promise<ListPage>;
  read(id: string): Promise<RemoteBody>;
  remove(id: string): Promise<void>;
  archive?: (id: string) => Promise<void>;
  conversationUrl(id: string): string;
}

/** `page` gives the adapter the few things it may read from the site's page; tests and sites that need none leave it out. */
export type AdapterFactory = (fetchJson: FetchJson, page?: PageAccess) => Adapter;
