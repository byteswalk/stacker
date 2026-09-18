export type SourceKind = "codex" | "claude_cli" | "claude_desktop" | "import";
export interface Source { id: string; name: string; kind: SourceKind; root: string; enabled: boolean }
export interface Conversation {
  id: string; native_id: string; source_id: string; client: string; path: string;
  title: string; project: string; parent_id: string; modified: number; bytes: number;
  fingerprint: string; archived: boolean; complete: boolean; warning: string;
  message_count: number; favorite: boolean; hidden: boolean; group_name: string;
  summary: string; summary_fingerprint: string;
}
export interface Message { line: number; role: string; text: string }
export interface Detail { conversation: Conversation; messages: Message[]; total: number }
export interface Query { search: string; source: string; project: string; state: string; before: number; offset: number; full_text: boolean }
export interface Listing { items: Conversation[]; total: number; ids: string[]; projects: string[]; indexed: number; synced_at: string; warnings: string[] }
export interface JobItem { id: string; title: string; status: string; detail: string }
export interface Job { id: string; action: string; state: string; done: number; total: number; items: JobItem[]; output: string; error: string }
export interface Preview { token: string; action: string; selected: string[]; affected: Conversation[]; blocked: JobItem[]; bytes: number }
export interface Settings { sources: Source[]; storage: string; model: { endpoint: string; model: string; has_key: boolean } }
export interface SummaryApproval { token: string; endpoint: string; model: string; items: { id: string; title: string; chars: number; text: string }[] }

export const EMPTY_LIST: Listing = { items: [], ids: [], projects: [], total: 0, indexed: 0, synced_at: "", warnings: [] };
export const EMPTY_QUERY: Query = { search: "", source: "", project: "", state: "", before: 0, offset: 0, full_text: false };
export function toggleSelection(current: string[], id: string): string[] {
  return current.includes(id) ? current.filter((value) => value !== id) : [...current, id];
}
export function currentSelection(selected: string[], ids: string[]): string[] {
  const available = new Set(ids); return selected.filter((id) => available.has(id));
}
export function isSummaryStale(c: Conversation): boolean { return !!c.summary && c.summary_fingerprint !== c.fingerprint; }
