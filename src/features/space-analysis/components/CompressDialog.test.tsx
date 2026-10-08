// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "../../../invoke";
import { CompressDialog, mediaKind } from "./CompressDialog";

vi.mock("../../../invoke", () => ({ invoke: vi.fn(), reportFrontendWarning: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const files = [
  { path: String.raw`D:\v\trip.mov`, bytes: 40_000_000 },
  { path: String.raw`D:\p\a.jpg`, bytes: 700_000 },
  { path: String.raw`D:\l\server.log`, bytes: 9_800_000 },
];

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  localStorage.clear();
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation((async (command: string, args?: { targets: typeof files }) => {
    if (command === "space_media_tools") return { ffmpeg: "ffmpeg.exe", version: "", videoEncoders: ["x265", "qsv"], imageFormats: ["jpeg", "webp"] };
    if (command === "space_compress_estimate" || command === "space_compress_run") {
      return args!.targets.map((t) => ({
        path: t.path, newPath: t.path, before: t.bytes, after: Math.round(t.bytes / 4),
        method: mediaKind(t.path), status: command === "space_compress_run" ? "ok" : "estimated",
      }));
    }
    return null;
  }) as typeof invoke);
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

const button = (text: string) => [...document.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.includes(text))!;

describe("compressing picked files", () => {
  it("sorts the files by what can be done with them", () => {
    expect(files.map((f) => mediaKind(f.path))).toEqual(["video", "image", "packed"]);
  });

  it("estimates with the chosen settings, then compresses and reports what was saved", async () => {
    const onDone = vi.fn();
    await act(async () => { root.render(<CompressDialog files={files} onClose={() => {}} onDone={onDone} />); });
    const text = () => document.body.textContent ?? "";
    expect(text()).toContain("视频 1 个，图片 1 个，其他 1 个");
    await act(async () => { button("省空间").click(); });
    await act(async () => { button("WebP").click(); });
    await act(async () => { button("估算").click(); });
    expect(invoke).toHaveBeenCalledWith("space_compress_estimate", {
      targets: files,
      options: { videoQuality: "small", videoHeight: 0, videoEncoder: "x265", imageFormat: "webp", imageQuality: "standard" },
    });
    expect(text()).toContain("预计共省出");
    await act(async () => { button("开始压缩").click(); });
    expect(onDone).toHaveBeenCalledTimes(1);
    expect(text()).toContain("共省出");
  });

  it("offers ffmpeg when videos or photos are picked and it is missing", async () => {
    vi.mocked(invoke).mockImplementation((async (command: string) => command === "space_media_tools"
      ? { ffmpeg: null, version: "", videoEncoders: [], imageFormats: [] } : null) as typeof invoke);
    await act(async () => { root.render(<CompressDialog files={files.slice(0, 1)} onClose={() => {}} onDone={() => {}} />); });
    expect(button("下载 ffmpeg")).toBeTruthy();
  });
});
