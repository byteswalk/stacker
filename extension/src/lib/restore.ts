import { applyBackup, type Backup, type Db, type RestoreCounts } from "./db";

export type Pull = (request: { section: keyof Backup; offset: number }) => Promise<unknown>;
interface PullPage { items: unknown[]; next: number | null }

/** Every item of one section, page by page; stops if Stacker's paging does not move forward. */
export async function pullSection(pull: Pull, section: keyof Backup): Promise<unknown[]> {
  const out: unknown[] = [];
  let offset = 0;
  for (;;) {
    const page = (await pull({ section, offset })) as PullPage;
    out.push(...page.items);
    if (page.next === null || page.next <= offset) return out;
    offset = page.next;
  }
}

/** 「从 Stacker 恢复」: brings back aliases, folders, conversations' local fields and excerpts. */
export async function restoreFromStacker(db: Db, pull: Pull): Promise<RestoreCounts> {
  const backup = {
    accounts: await pullSection(pull, "accounts"),
    folders: await pullSection(pull, "folders"),
    conversations: await pullSection(pull, "conversations"),
    excerpts: await pullSection(pull, "excerpts"),
  } as Backup;
  return applyBackup(db, backup);
}
