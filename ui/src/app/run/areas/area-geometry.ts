import { AreaBox, AreaRect } from '../../api';

/** An area, its kind or not. */
export type AreaBounds = AreaRect | AreaBox;

/** A point on the video, as shares of its width and height. */
export type SharePoint = [x: number, y: number];

/** The video's size on screen, in pixels. */
export interface ScreenSize {
  width: number;
  height: number;
}

/** The edges of an area the pointer holds: one, or two at a corner. */
export interface HeldEdges {
  left: boolean;
  right: boolean;
  top: boolean;
  bottom: boolean;
}

/** Where the pointer took hold of an area: which one, and its edges (null: its inside, to move it). */
export interface AreaHold {
  index: number;
  edges: HeldEdges | null;
}

/** How near an edge (pixels) the pointer resizes, rather than moves, an area. */
const EDGE_PX = 6;
/** An area's smallest width and height when resized, in pixels. */
const MIN_PX = 8;
/** How far (pixels) the pointer goes before a press on an area moves it, rather than selects it. */
export const DRAG_PX = 3;
/** A drag on the empty video shorter than this (pixels) both ways is a click: it selects nothing. */
const CLICK_PX = 6;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** The area under the point and the edges held there: the selected area first, then the topmost (the last drawn). */
export function holdAt(
  boxes: readonly AreaBounds[],
  selected: number,
  [x, y]: SharePoint,
  size: ScreenSize,
): AreaHold | null {
  const px = EDGE_PX / size.width;
  const py = EDGE_PX / size.height;
  const order = [...boxes.keys()].reverse();
  if (selected >= 0 && selected < boxes.length) order.unshift(selected);
  for (const index of order) {
    const [a, b, c, d] = boxes[index];
    if (x < a - px || x > c + px || y < b - py || y > d + py) continue;
    const edges: HeldEdges = {
      left: Math.abs(x - a) <= px,
      right: Math.abs(x - c) <= px,
      top: Math.abs(y - b) <= py,
      bottom: Math.abs(y - d) <= py,
    };
    const onEdge = edges.left || edges.right || edges.top || edges.bottom;
    return { index, edges: onEdge ? edges : null };
  }
  return null;
}

/** The pointer's shape over the video: draw a new area, move one, or resize one by its edge or corner. */
export function cursorFor(hold: AreaHold | null): string {
  if (!hold) return 'crosshair';
  if (!hold.edges) return 'move';
  const { left, right, top, bottom } = hold.edges;
  if ((left && top) || (right && bottom)) return 'nwse-resize';
  if ((right && top) || (left && bottom)) return 'nesw-resize';
  return left || right ? 'ew-resize' : 'ns-resize';
}

/**
 * An area dragged from where it was by [dx, dy] (shares): moved and kept on screen, or with the held edges moved and
 * kept at least MIN_PX across.
 */
export function dragArea(
  [a, b, c, d]: AreaRect,
  edges: HeldEdges | null,
  [dx, dy]: SharePoint,
  size: ScreenSize,
): AreaRect {
  if (!edges) {
    const ox = clamp(dx, -a, 1 - c);
    const oy = clamp(dy, -b, 1 - d);
    return [a + ox, b + oy, c + ox, d + oy];
  }
  const mw = MIN_PX / size.width;
  const mh = MIN_PX / size.height;
  return [
    edges.left ? clamp(a + dx, 0, c - mw) : a,
    edges.top ? clamp(b + dy, 0, d - mh) : b,
    edges.right ? clamp(c + dx, a + mw, 1) : c,
    edges.bottom ? clamp(d + dy, b + mh, 1) : d,
  ];
}

/** Areas overlap: the next one under the point after `from`, going down the stack; -1 when there is none. */
export function nextUnder(boxes: readonly AreaBounds[], from: number, [x, y]: SharePoint): number {
  const n = boxes.length;
  for (let k = 1; k < n; k++) {
    const j = (from - k + n) % n;
    const [a, b, c, d] = boxes[j];
    if (x >= a && x <= c && y >= b && y <= d) return j;
  }
  return -1;
}

/** The area a drag on the empty video draws, kept on screen; null for a drag so short it is a click. */
export function drawnArea(
  [x0, y0]: SharePoint,
  [x1, y1]: SharePoint,
  size: ScreenSize,
): AreaRect | null {
  if (Math.abs(x1 - x0) * size.width < CLICK_PX && Math.abs(y1 - y0) * size.height < CLICK_PX) {
    return null;
  }
  const c = (v: number) => clamp(v, 0, 1);
  return [c(Math.min(x0, x1)), c(Math.min(y0, y1)), c(Math.max(x0, x1)), c(Math.max(y0, y1))];
}

/** Whether two lists of areas cover the same parts of the frame (their kinds aside). */
export function sameAreas(a: readonly AreaBounds[], b: readonly AreaBounds[]): boolean {
  const near = (p: AreaBounds, q: AreaBounds) =>
    [0, 1, 2, 3].every((k) => Math.abs(Number(p[k]) - Number(q[k])) < 1e-9);
  return a.length === b.length && a.every((box, i) => near(box, b[i]));
}

/** How the areas are drawn: their colors, lines and label font, from the areas' tokens (themes/areas.scss). */
export interface AreaStyle {
  fill: string;
  fillSelected: string;
  line: string;
  lineSelected: string;
  lineWidth: number;
  lineWidthSelected: number;
  labelFont: string;
  labelBg: string;
  labelText: string;
  labelTextSelected: string;
}

export function readAreaStyle(el: Element): AreaStyle {
  const css = getComputedStyle(el);
  const v = (name: string) => css.getPropertyValue(name).trim();
  return {
    fill: v('--area-fill'),
    fillSelected: v('--area-fill-selected'),
    line: v('--area-line'),
    lineSelected: v('--area-line-selected'),
    lineWidth: Number(v('--area-line-width')),
    lineWidthSelected: Number(v('--area-line-width-selected')),
    labelFont: v('--area-label-font'),
    labelBg: v('--area-label-bg'),
    labelText: v('--area-label-text'),
    labelTextSelected: v('--area-label-text-selected'),
  };
}

/** What the editor draws: the areas, the selected one, the one being drawn, and the one whose label is hidden. */
export interface AreaScene {
  boxes: readonly AreaBox[];
  selected: number;
  drawing: AreaRect | null;
  /** The area being dragged: its label is hidden while it moves. */
  moving: number;
  kindName: (id: string) => string;
}

/** A label's inset from the area's corner, its padding and its height, in pixels. */
const LABEL_INSET = 2;
const LABEL_PAD = 4;
const LABEL_HEIGHT = 16;

/**
 * The areas over the video, each with its kind's name: plain to read on the others, see-through on the selected one
 * (its corner must stay visible), hidden while it is dragged. The area being drawn has no name yet.
 */
export function drawAreas(
  c: CanvasRenderingContext2D,
  scene: AreaScene,
  size: ScreenSize,
  st: AreaStyle,
): void {
  const all: AreaBox[] = [...scene.boxes];
  if (scene.drawing) all.push([...scene.drawing, '']);
  all.forEach(([x0, y0, x1, y1, kind], i) => {
    const x = x0 * size.width;
    const y = y0 * size.height;
    const w = (x1 - x0) * size.width;
    const h = (y1 - y0) * size.height;
    const sel = i === scene.selected;
    c.fillStyle = sel ? st.fillSelected : st.fill;
    c.fillRect(x, y, w, h);
    c.strokeStyle = sel ? st.lineSelected : st.line;
    c.lineWidth = sel ? st.lineWidthSelected : st.lineWidth;
    c.strokeRect(x + 0.5, y + 0.5, w - 1, h - 1);
    if (!kind || i === scene.moving) return;
    const name = scene.kindName(kind);
    c.font = st.labelFont;
    c.textBaseline = 'middle';
    c.save();
    c.beginPath();
    c.rect(x + LABEL_INSET, y + LABEL_INSET, w - 2 * LABEL_INSET, LABEL_HEIGHT);
    c.clip();
    if (!sel) {
      c.fillStyle = st.labelBg;
      c.fillRect(
        x + LABEL_INSET,
        y + LABEL_INSET,
        c.measureText(name).width + 2 * LABEL_PAD,
        LABEL_HEIGHT,
      );
    }
    c.fillStyle = sel ? st.labelTextSelected : st.labelText;
    c.fillText(name, x + LABEL_INSET + LABEL_PAD, y + LABEL_INSET + LABEL_HEIGHT / 2);
    c.restore();
  });
}
