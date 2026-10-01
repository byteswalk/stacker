import { describe, expect, it } from "vitest";
import { smartPick } from "./DuplicateFiles";

const group = (paths: string[], verified = true) => ({ bytes: 100, wasted: 100, paths, verified });
const none = new Set<string>();

describe("smart pick for duplicates", () => {
  it("never picks a program's own files", () => {
    // Codex's binary in two installs: identical, and both needed.
    expect(smartPick(group([
      String.raw`C:\Users\me\.codex\plugins\.plugin-appserver\codex.exe`,
      String.raw`C:\Users\me\AppData\Local\OpenAI\Codex\bin\codex.exe`,
    ]), none)).toEqual([]);
  });

  it("keeps one copy of the user's own files and picks the rest", () => {
    const picked = smartPick(group([
      String.raw`C:\Users\me\Documents\report.pdf`,
      String.raw`C:\Users\me\Downloads\report (1).pdf`,
    ]), none);
    expect(picked).toEqual([String.raw`C:\Users\me\Downloads\report (1).pdf`]);
  });

  it("picks every spare when a copy that stays is elsewhere", () => {
    expect(smartPick(group([
      String.raw`C:\Users\me\AppData\Local\updater\installer.exe`,
      String.raw`C:\Users\me\AppData\Local\updater\pending\setup.exe`,
    ]), none)).toEqual([String.raw`C:\Users\me\AppData\Local\updater\pending\setup.exe`]);
  });

  it("leaves system files and groups compared only by their ends alone", () => {
    const paths = [String.raw`C:\Windows\Installer\a.msi`, String.raw`C:\Users\me\Downloads\a.msi`];
    expect(smartPick(group(paths), new Set([paths[0]]))).toEqual([paths[1]]);
    expect(smartPick(group([String.raw`C:\Users\me\Downloads\a.iso`, String.raw`C:\Users\me\Desktop\a.iso`], false), none)).toEqual([]);
  });
});
