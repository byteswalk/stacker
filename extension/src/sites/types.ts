import type { ListPage, RemoteAccount, RemoteBody, SiteId } from "../shared/types";
import type { FetchJson } from "./http";

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

export type AdapterFactory = (fetchJson: FetchJson) => Adapter;
