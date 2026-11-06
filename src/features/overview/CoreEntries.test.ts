import { describe, expect, it } from "vitest";
import { agentTally, diskTone, logoTools, rankedVolumes, ringDash, ringVolumes, sessionShares, vaultTally } from "./CoreEntries";
import type { VibeSurface } from "../agents/catalogStore";

const GB = 1024 ** 3;
const surface = (installed: boolean, update_available = false) => ({ installed, update_available }) as VibeSurface;
const tool = (id: string, family_id: string, sort_order: number, installed: boolean, icon?: string) =>
  ({ id, name: id, family_id, sort_order, icon, cli: surface(installed), desktop: surface(false) });

describe("core entries", () => {
  it("counts an agent once however many surfaces it has, and updates one by one", () => {
    const tally = agentTally([
      { cli: surface(true, true), desktop: surface(true) },
      { cli: surface(true), desktop: null },
      // Installed nowhere: counted in the total, not in what is installed.
      { cli: surface(false), desktop: surface(false) },
    ]);
    expect(tally).toEqual({ installed: 2, total: 3, updates: 1 });
  });

  it("shows one logo per product, the ones in use first", () => {
    const logos = logoTools([
      tool("a", "a", 1, false, "a.png"),
      tool("b-cn", "b", 2, false, "b.png"),
      tool("b-global", "b", 3, false, "b.png"),
      tool("c", "c", 4, true, "c.png"),
      tool("d", "d", 5, true),
    ]);
    expect(logos.map((logo) => [logo.id, logo.on])).toEqual([["c", true], ["a", false], ["b-cn", false]]);
  });

  it("keeps nineteen logos at most, the card shows the rest as a count", () => {
    const many = Array.from({ length: 27 }, (_, index) => tool(`t${index}`, `t${index}`, index, false, "x.png"));
    expect(logoTools(many)).toHaveLength(19);
  });

  it("marks a logo with an update waiting", () => {
    const logo = logoTools([{ ...tool("a", "a", 1, true, "a.png"), cli: surface(true, true) }])[0];
    expect(logo.update).toBe(true);
    expect(logoTools([tool("b", "b", 1, true, "b.png")])[0].update).toBe(false);
  });

  it("gives the biggest users of the session data their share", () => {
    const agent = (name: string, total: number) => ({ product: { id: name, name, icon: "", sessionsAgent: null }, total, reclaimable: 0, items: [] });
    const shares = sessionShares({ total: 100, agents: [agent("a", 10), agent("b", 60), agent("c", 0), agent("d", 30)] }, 2);
    expect(shares).toEqual([{ name: "b", bytes: 60, percent: 60 }, { name: "d", bytes: 30, percent: 30 }]);
    expect(sessionShares({ total: 0, agents: [] })).toEqual([]);
  });

  it("puts the disk with the least room first, not the biggest one", () => {
    const disks = rankedVolumes([
      // 90% used, yet 400 GB left: nothing to worry about.
      { root: "K:\\", totalBytes: 4000 * GB, freeBytes: 400 * GB, fixed: true },
      { root: "C:\\", totalBytes: 500 * GB, freeBytes: 20 * GB, fixed: true },
      // Removable drives are somebody else's problem.
      { root: "E:\\", totalBytes: 100 * GB, freeBytes: 1 * GB, fixed: false },
    ]);
    expect(disks.map((disk) => [disk.root, disk.tone])).toEqual([["C:", "warn"], ["K:", "ok"]]);
    expect(rankedVolumes([])).toEqual([]);
  });

  it("keeps the tightest disks and lists them by drive letter", () => {
    const disk = (root: string, free: number) => ({ root: `${root}:\\`, totalBytes: 500 * GB, freeBytes: free * GB, fixed: true });
    const ranked = rankedVolumes([disk("K", 1), disk("C", 300), disk("F", 5), disk("D", 400), disk("E", 450)]);
    expect(ringVolumes(ranked).map((item) => item.root)).toEqual(["C:", "F:", "K:"]);
    // A wider card shows more of them, still the fullest first and in letter order.
    expect(ringVolumes(ranked, 4).map((item) => item.root)).toEqual(["C:", "D:", "F:", "K:"]);
    expect(ringVolumes(ranked, 9)).toHaveLength(5);
    expect(ringVolumes(ranked, 0)).toHaveLength(1);
  });

  it("draws a ring in proportion, never past a full turn", () => {
    expect(ringDash(0.5, 10)).toBe("31.42 62.83");
    expect(ringDash(1.2, 10)).toBe("62.83 62.83");
    expect(ringDash(0, 10)).toBe("0.00 62.83");
  });

  it("counts the vault by kind, and what runs out within a month", () => {
    const tally = vaultTally([
      { kind: "api_key", expiresAt: "2026-11-20" },
      { kind: "other", expiresAt: "2027-06-01" },
      { kind: "ssh_key", expiresAt: null },
      // Already past its date: still something to deal with.
      { kind: "token", expiresAt: "2026-01-01" },
    ], new Date("2026-11-01T00:00:00Z"));
    expect(tally).toEqual({ total: 4, credentials: 3, ssh: 1, expiring: 2 });
  });

  it("judges a disk by the room it has left", () => {
    expect(diskTone(400 * GB, 0.9)).toBe("ok");
    expect(diskTone(50 * GB, 0.96)).toBe("warn");
    expect(diskTone(20 * GB, 0.5)).toBe("warn");
    expect(diskTone(5 * GB, 0.5)).toBe("bad");
  });
});
