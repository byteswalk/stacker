// @vitest-environment jsdom
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { setInArray, setInSet, useDragPick } from "./dragPick";

/** A plain list: rows a–e, "c" cannot be picked. */
function List() {
  const ids = ["a", "b", "c", "d", "e"];
  const [picked, setPicked] = useState<string[]>([]);
  const pick = useDragPick(ids.map((id) => id === "c" ? [] : [id]), (id) => picked.includes(id), (rows, on) => setPicked((old) => setInArray(old, rows, on)));
  return <div>{ids.map((id, i) => <div key={id} className="row" data-on={picked.includes(id)} {...pick.row(i)}>
    <input type="checkbox" checked={picked.includes(id)} disabled={id === "c"} {...pick.box(i)} onChange={(e) => pick.change(i, e.target.checked)} />
  </div>)}</div>;
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

const rows = () => [...host.querySelectorAll<HTMLElement>(".row")];
const boxes = () => [...host.querySelectorAll<HTMLInputElement>("input")];
const on = () => rows().map((row) => row.dataset.on === "true");
const fire = async (target: EventTarget, type: string, init: MouseEventInit = {}) => {
  await act(async () => { target.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, button: 0, ...init })); });
};
// React hears a row being entered from the pointer leaving the one before.
const drag = async (from: number, to: number) => {
  await fire(boxes()[from], "pointerdown");
  for (let i = from; i !== to; i += Math.sign(to - from)) await fire(rows()[i], "pointerout", { relatedTarget: rows()[i + Math.sign(to - from)] });
  await fire(window, "pointerup");
};

describe("picking rows by dragging", () => {
  it("sets every row passed the way the first one went, and skips what cannot be picked", async () => {
    await act(async () => { root.render(<List />); });
    await drag(0, 4);
    expect(on()).toEqual([true, true, false, true, true]);
    await drag(3, 1);
    expect(on()).toEqual([true, false, false, false, true]);
  });

  it("picks a range with shift, and the click after a press changes nothing more", async () => {
    await act(async () => { root.render(<List />); });
    await fire(boxes()[0], "pointerdown");
    await fire(window, "pointerup");
    await act(async () => { boxes()[0].click(); });
    expect(on()).toEqual([true, false, false, false, false]);
    await fire(boxes()[3], "pointerdown", { shiftKey: true });
    await fire(window, "pointerup");
    expect(on()).toEqual([true, true, false, true, false]);
  });

  it("keeps lists and sets free of duplicates", () => {
    expect(setInArray(["a"], ["a", "b"], true)).toEqual(["a", "b"]);
    expect(setInArray(["a", "b"], ["a"], false)).toEqual(["b"]);
    expect([...setInSet(new Set(["a"]), ["b"], true)]).toEqual(["a", "b"]);
  });
});
