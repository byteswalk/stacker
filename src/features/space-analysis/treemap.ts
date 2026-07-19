export interface TreemapDatum {
  id: string;
  value: number;
}

export interface TreemapRect extends TreemapDatum {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface AreaDatum extends TreemapDatum {
  area: number;
  order: number;
}

interface Bounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

function worstAspectRatio(row: readonly AreaDatum[], side: number): number {
  if (row.length === 0 || side <= 0) return Number.POSITIVE_INFINITY;
  const sum = row.reduce((total, item) => total + item.area, 0);
  const maximum = Math.max(...row.map((item) => item.area));
  const minimum = Math.min(...row.map((item) => item.area));
  const sideSquared = side * side;
  const sumSquared = sum * sum;
  return Math.max(
    (sideSquared * maximum) / sumSquared,
    sumSquared / (sideSquared * minimum),
  );
}

function layoutRow(row: readonly AreaDatum[], bounds: Bounds): {
  rectangles: TreemapRect[];
  remaining: Bounds;
} {
  const area = row.reduce((sum, item) => sum + item.area, 0);
  const rectangles: TreemapRect[] = [];

  if (bounds.width >= bounds.height) {
    const rowWidth = bounds.height > 0 ? area / bounds.height : 0;
    let cursor = bounds.y;
    row.forEach((item, index) => {
      const height = index === row.length - 1
        ? Math.max(0, bounds.y + bounds.height - cursor)
        : item.area / rowWidth;
      rectangles.push({
        id: item.id,
        value: item.value,
        x: bounds.x,
        y: cursor,
        width: rowWidth,
        height,
      });
      cursor += height;
    });
    return {
      rectangles,
      remaining: {
        x: bounds.x + rowWidth,
        y: bounds.y,
        width: Math.max(0, bounds.width - rowWidth),
        height: bounds.height,
      },
    };
  }

  const rowHeight = bounds.width > 0 ? area / bounds.width : 0;
  let cursor = bounds.x;
  row.forEach((item, index) => {
    const width = index === row.length - 1
      ? Math.max(0, bounds.x + bounds.width - cursor)
      : item.area / rowHeight;
    rectangles.push({
      id: item.id,
      value: item.value,
      x: cursor,
      y: bounds.y,
      width,
      height: rowHeight,
    });
    cursor += width;
  });
  return {
    rectangles,
    remaining: {
      x: bounds.x,
      y: bounds.y + rowHeight,
      width: bounds.width,
      height: Math.max(0, bounds.height - rowHeight),
    },
  };
}

/** Deterministic squarified treemap using absolute pixel coordinates. */
export function layoutTreemap(
  data: readonly TreemapDatum[],
  width: number,
  height: number,
  sort: "value" | "input" = "value",
): TreemapRect[] {
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
    return [];
  }

  const values = data
    .map((item, order) => ({ ...item, order }))
    .filter((item) => Number.isFinite(item.value) && item.value > 0)
    .sort((left, right) => sort === "input" ? left.order - right.order : right.value - left.value || left.order - right.order);
  const total = values.reduce((sum, item) => sum + item.value, 0);
  if (total <= 0) return [];

  const scale = (width * height) / total;
  const remainingItems: AreaDatum[] = values.map((item) => ({
    ...item,
    area: item.value * scale,
  }));
  const rectangles: TreemapRect[] = [];
  let bounds: Bounds = { x: 0, y: 0, width, height };

  while (remainingItems.length > 0 && bounds.width > 0 && bounds.height > 0) {
    const row: AreaDatum[] = [remainingItems.shift()!];
    const side = Math.min(bounds.width, bounds.height);
    while (remainingItems.length > 0) {
      const candidate = remainingItems[0];
      if (worstAspectRatio([...row, candidate], side) > worstAspectRatio(row, side)) break;
      row.push(remainingItems.shift()!);
    }
    const laidOut = layoutRow(row, bounds);
    rectangles.push(...laidOut.rectangles);
    bounds = laidOut.remaining;
  }

  return rectangles;
}

export function intersectionArea(left: TreemapRect, right: TreemapRect): number {
  const width = Math.max(0, Math.min(left.x + left.width, right.x + right.width) - Math.max(left.x, right.x));
  const height = Math.max(0, Math.min(left.y + left.height, right.y + right.height) - Math.max(left.y, right.y));
  return width * height;
}
