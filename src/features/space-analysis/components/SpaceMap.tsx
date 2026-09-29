import { useCallback, useEffect, useState } from "react";
import { invoke } from "../../../invoke";
import { useI18n } from "../../../i18n";
import { useToast } from "../../../ui";
import { formatSpaceBytes as bytes } from "./SpaceOverview";
import type { DirectoryNode } from "../types";

export type Tile = { node: DirectoryNode; x: number; y: number; width: number; height: number };

/**
 * Squarified treemap: each row of tiles is filled until adding another would make the tiles
 * longer and thinner than they already are, which keeps them close to square and readable.
 */
export function layout(nodes: DirectoryNode[], width: number, height: number): Tile[] {
  const sized = nodes.filter((node) => node.allocatedBytes > 0);
  const total = sized.reduce((sum, node) => sum + node.allocatedBytes, 0);
  if (!sized.length || total <= 0 || width <= 0 || height <= 0) return [];
  const area = (node: DirectoryNode) => (node.allocatedBytes / total) * width * height;
  const tiles: Tile[] = [];
  let [x, y, w, h] = [0, 0, width, height];
  let row: DirectoryNode[] = [];

  /** How square the worst tile in a row would be; lower is better. */
  const worst = (candidates: DirectoryNode[], side: number) => {
    const sum = candidates.reduce((total, node) => total + area(node), 0);
    if (sum <= 0) return Infinity;
    const max = Math.max(...candidates.map(area));
    const min = Math.min(...candidates.map(area));
    return Math.max((side * side * max) / (sum * sum), (sum * sum) / (side * side * min));
  };

  const flush = () => {
    const sum = row.reduce((total, node) => total + area(node), 0);
    const vertical = w >= h;
    const thickness = sum / (vertical ? h : w);
    let offset = 0;
    for (const node of row) {
      const length = area(node) / thickness;
      tiles.push(vertical
        ? { node, x, y: y + offset, width: thickness, height: length }
        : { node, x: x + offset, y, width: length, height: thickness });
      offset += length;
    }
    if (vertical) { x += thickness; w -= thickness; } else { y += thickness; h -= thickness; }
    row = [];
  };

  for (const node of [...sized].sort((a, b) => b.allocatedBytes - a.allocatedBytes)) {
    const side = Math.min(w, h);
    if (row.length && worst([...row, node], side) > worst(row, side)) flush();
    row.push(node);
  }
  if (row.length) flush();
  return tiles;
}

const COLOURS = ["#5b8def", "#6bcf86", "#e6b450", "#c98ae0", "#57c6d6", "#e2625b", "#8fa8e0", "#7fd1c6"];

/** The scanned tree as rectangles: the shape of the disk, not a list of numbers. */
export function SpaceMap({ taskId, roots }: { taskId: string; roots: DirectoryNode[] }) {
  const { tr: t } = useI18n();
  const toast = useToast();
  const [trail, setTrail] = useState<DirectoryNode[]>([]);
  const [children, setChildren] = useState<DirectoryNode[] | null>(null);

  const current = trail[trail.length - 1] ?? null;

  const load = useCallback(async (node: DirectoryNode | null) => {
    if (!node) { setChildren(roots); return; }
    try {
      const page = await invoke<{ items: DirectoryNode[] }>("space_scan_children", { taskId, parentId: node.nodeId, offset: 0, limit: 200 });
      setChildren(page.items);
    } catch (e) { toast(String(e), "err"); setChildren([]); }
  }, [roots, taskId, toast]);

  useEffect(() => { void load(current); }, [current, load]);

  const width = 1000;
  const height = 420;
  const tiles = layout(children ?? [], width, height);

  return <div className="spacemap">
    <div className="spacemap-trail">
      <button className="gh xs" onClick={() => setTrail([])} disabled={!trail.length}><i className="ti ti-home" /></button>
      {trail.map((node, index) => <button className="gh xs" key={node.nodeId} onClick={() => setTrail(trail.slice(0, index + 1))}>{node.name}</button>)}
      <span className="s dim">{t("点方块进入下一层，方块面积就是占用大小")}</span>
    </div>
    {!tiles.length
      ? <div className="space-analysis-state"><i className="ti ti-square-off" /><span>{t("这一层没有可显示的目录")}</span></div>
      : <svg className="spacemap-svg" viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" role="img">
        {tiles.map((tile, index) => <g key={tile.node.nodeId}
          onClick={() => tile.node.childCount > 0 && setTrail([...trail, tile.node])}
          className={tile.node.childCount > 0 ? "on" : ""}>
          <title>{`${tile.node.path}\n${bytes(tile.node.allocatedBytes)}`}</title>
          <rect x={tile.x} y={tile.y} width={Math.max(0, tile.width - 2)} height={Math.max(0, tile.height - 2)}
            fill={COLOURS[index % COLOURS.length]} fillOpacity={0.22} stroke={COLOURS[index % COLOURS.length]} strokeOpacity={0.55} />
          {tile.width > 90 && tile.height > 34 && <>
            <text x={tile.x + 9} y={tile.y + 20} className="spacemap-name">{tile.node.name}</text>
            <text x={tile.x + 9} y={tile.y + 36} className="spacemap-size">{bytes(tile.node.allocatedBytes)}</text>
          </>}
        </g>)}
      </svg>}
  </div>;
}
