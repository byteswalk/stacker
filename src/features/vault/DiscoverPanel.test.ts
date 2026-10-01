import { describe, expect, it } from "vitest";
import type { Finding } from "./api";
import { groupFindings, locationText } from "./DiscoverPanel";

function finding(id: number, source: Finding["source"], location: string): Finding {
  return { id, source, location, name: `n${id}`, preview: "abcd•••", platform: "", kind: "api_key", risks: [], status: "new" };
}

describe("discover panel helpers", () => {
  it("groups findings by source in a fixed order", () => {
    const groups = groupFindings([finding(0, "dotenv", "D:\\p\\.env"), finding(1, "ssh", "C:\\u\\.ssh\\id"), finding(2, "dotenv", "D:\\q\\.env")]);
    expect(groups.map(([source, items]) => [source, items.map((item) => item.id)])).toEqual([["ssh", [1]], ["dotenv", [0, 2]]]);
  });

  it("names environment scopes instead of showing raw codes", () => {
    expect(locationText(finding(0, "env", "user"))).toBe("用户环境变量");
    expect(locationText(finding(0, "env", "system"))).toBe("系统环境变量");
    expect(locationText(finding(0, "config", "C:\\u\\.npmrc"))).toBe("C:\\u\\.npmrc");
  });
});
