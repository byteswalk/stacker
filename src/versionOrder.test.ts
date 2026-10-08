import { describe, expect, it } from "vitest";
import { compareVersions, newestFirst, rustToolchainOrder } from "./versionOrder";

describe("version order", () => {
  it("puts the newest version on top by its numbers, not its spelling", () => {
    expect(newestFirst(["3.10.11", "3.12.10", "3.13.14", "3.14.8", "3.9.6"], (v) => v))
      .toEqual(["3.14.8", "3.13.14", "3.12.10", "3.10.11", "3.9.6"]);
    expect(newestFirst(["v20.18.0", "v22.11.0", "v8.17.0"], (v) => v)).toEqual(["v22.11.0", "v20.18.0", "v8.17.0"]);
    expect(newestFirst(["9.8.0", "10.0.0", "9.10.1"], (v) => v)).toEqual(["10.0.0", "9.10.1", "9.8.0"]);
  });

  it("reads Java 8 written as 1.8 as 8", () => {
    expect(newestFirst(["1.8.0_412", "21.0.4", "11.0.24"], (v) => v)).toEqual(["21.0.4", "11.0.24", "1.8.0_412"]);
    expect(compareVersions("go1.23.4", "go1.22.10")).toBeGreaterThan(0);
  });

  it("lists Rust's channels first, then numbered toolchains newest first", () => {
    const names = ["1.80.0-x86_64-pc-windows-msvc", "nightly-x86_64-pc-windows-msvc", "stable-x86_64-pc-windows-msvc", "1.89.0-x86_64-pc-windows-msvc"];
    expect(rustToolchainOrder(names, (n) => n)).toEqual([
      "stable-x86_64-pc-windows-msvc", "nightly-x86_64-pc-windows-msvc",
      "1.89.0-x86_64-pc-windows-msvc", "1.80.0-x86_64-pc-windows-msvc",
    ]);
  });
});
