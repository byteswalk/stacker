// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { templateOf, useColumns, type Column } from "./columns";

const COLUMNS: Column[] = [
  { key: "pick", track: "16px" },
  { key: "title", track: "minmax(0,1.6fr)", resizable: true, min: 80 },
  { key: "site", track: "minmax(0,1fr)", resizable: true, min: 80 },
  { key: "ops", track: "auto", grows: true, min: 300 },
];

describe("resizable columns", () => {
  it("caps a dragged column instead of fixing it, and lets the actions take what is left", () => {
    expect(templateOf(COLUMNS, {})).toBe("16px minmax(0,1.6fr) minmax(0,1fr) auto");
    // One still stretches: the actions keep their own width.
    expect(templateOf(COLUMNS, { title: 320 })).toBe("16px minmax(80px, 320px) minmax(0,1fr) auto");
    // None stretches any more: the room left goes to the actions, never past the box.
    expect(templateOf(COLUMNS, { title: 320, site: 200 })).toBe("16px minmax(80px, 320px) minmax(80px, 200px) minmax(auto, 1fr)");
    // A width for a column that cannot be dragged is ignored.
    expect(templateOf(COLUMNS, { pick: 99 })).toBe("16px minmax(0,1.6fr) minmax(0,1fr) auto");
  });

  describe("in a list", () => {
    let host: HTMLDivElement;
    let root: Root;
    beforeEach(() => {
      Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
      localStorage.setItem("test.columns", JSON.stringify({ title: 300 }));
      host = document.createElement("div"); document.body.append(host); root = createRoot(host);
    });
    afterEach(() => { act(() => root.unmount()); host.remove(); localStorage.clear(); });

    function List() {
      const columns = useColumns("test.columns", COLUMNS);
      return <div data-columns="" data-template={columns.template}><span className="col-cell">title{columns.handle("title")}</span></div>;
    }

    it("remembers a width and forgets it on a double-click", async () => {
      await act(async () => { root.render(<List />); });
      const list = host.querySelector<HTMLElement>("[data-columns]")!;
      expect(list.dataset.template).toContain("minmax(80px, 300px)");
      await act(async () => { host.querySelector(".col-resize")!.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })); });
      expect(list.dataset.template).toBe("16px minmax(0,1.6fr) minmax(0,1fr) auto");
      expect(JSON.parse(localStorage.getItem("test.columns")!)).toEqual({});
    });
  });
});
