/**
 * The excluded areas editor's geometry and drawing: which area and edge the pointer holds, moving
 * and resizing an area, drawing a new one, comparing two lists of areas, and painting the areas on
 * the canvas. In: areas and points as shares of the frame, the video's size on screen, and the
 * areas' tokens (themes/areas.scss). Out: area-canvas (pointer and drawing) and AreaDraft
 * (`sameAreas`, whether to track again).
 */

import { AreaBox, AreaRect } from '../../api';

/** An area, its kind or not. */
export type AreaBounds = AreaRect | AreaBox;

/** A point on the video, as shares of its width and height. */
export type SharePoint = [x: number, y: number];

/** The video's size on screen, in pixels. */
export interface ScreenSize {
  /** The video's width on screen, in CSS pixels. */
  width: number;
  /** The video's height on screen, in CSS pixels. */
  height: number;
}

/** The edges of an area the pointer holds: one, or two at a corner. */
export interface HeldEdges {
  /** The pointer holds the left edge. */
  left: boolean;
  /** The pointer holds the right edge. */
  right: boolean;
  /** The pointer holds the top edge. */
  top: boolean;
  /** The pointer holds the bottom edge. */
  bottom: boolean;
}

/**
 * Where the pointer took hold of an area: which one, and its edges (null: its inside, to move it).
 */
export interface AreaHold {
  /** The area's index in the list. */
  index: number;
  /** The edges held, or null when the pointer is inside the area, away from its edges. */
  edges: HeldEdges | null;
}

/** How near an edge (pixels) the pointer resizes, rather than moves, an area. */
const EDGE_PX = 6;
/** An area's smallest width and height when resized, in pixels. */
const MIN_PX = 8;
/** How far (pixels) the pointer goes before a press on an area moves it, rather than selects it. */
export const DRAG_PX = 3;
/**
 * A drag on the empty video shorter than this (pixels) both ways is a click: it selects nothing.
 */
const CLICK_PX = 6;
/** Two areas whose edges are this close (shares of the video) cover the same part of it. */
const SAME_SHARE = 1e-9;

/** The value kept between low and high. */
const clamp = (value: number, low: number, high: number) => Math.min(high, Math.max(low, value));

/**
 * The area under the point and the edges held there: the selected area first, then the topmost (the
 * last drawn). A point within EDGE_PX of an area's edge counts as on it. Null when the point is on
 * no area.
 */
export function holdAt(
  boxes: readonly AreaBounds[],
  selected: number,
  [x, y]: SharePoint,
  size: ScreenSize,
): AreaHold | null {
  const reachX = EDGE_PX / size.width;
  const reachY = EDGE_PX / size.height;
  const order = [...boxes.keys()].reverse();
  if (selected >= 0 && selected < boxes.length) order.unshift(selected);
  for (const index of order) {
    const [left, top, right, bottom] = boxes[index];
    if (x < left - reachX || x > right + reachX || y < top - reachY || y > bottom + reachY)
      continue;
    const edges: HeldEdges = {
      left: Math.abs(x - left) <= reachX,
      right: Math.abs(x - right) <= reachX,
      top: Math.abs(y - top) <= reachY,
      bottom: Math.abs(y - bottom) <= reachY,
    };
    const onEdge = edges.left || edges.right || edges.top || edges.bottom;
    return { index, edges: onEdge ? edges : null };
  }
  return null;
}

/**
 * The CSS cursor over the video for what a press would do: draw a new area, move one, or resize one
 * by its edge or corner.
 */
export function cursorFor(hold: AreaHold | null): string {
  if (!hold) return 'crosshair';
  if (!hold.edges) return 'move';
  const { left, right, top, bottom } = hold.edges;
  if ((left && top) || (right && bottom)) return 'nwse-resize';
  if ((right && top) || (left && bottom)) return 'nesw-resize';
  return left || right ? 'ew-resize' : 'ns-resize';
}

/**
 * An area dragged from where it was by [dx, dy] (shares): moved and kept on screen, or with the
 * held edges moved and kept at least MIN_PX across.
 */
export function dragArea(
  [left, top, right, bottom]: AreaRect,
  edges: HeldEdges | null,
  [dx, dy]: SharePoint,
  size: ScreenSize,
): AreaRect {
  if (!edges) {
    const shiftX = clamp(dx, -left, 1 - right);
    const shiftY = clamp(dy, -top, 1 - bottom);
    return [left + shiftX, top + shiftY, right + shiftX, bottom + shiftY];
  }
  const minWidth = MIN_PX / size.width;
  const minHeight = MIN_PX / size.height;
  return [
    edges.left ? clamp(left + dx, 0, right - minWidth) : left,
    edges.top ? clamp(top + dy, 0, bottom - minHeight) : top,
    edges.right ? clamp(right + dx, left + minWidth, 1) : right,
    edges.bottom ? clamp(bottom + dy, top + minHeight, 1) : bottom,
  ];
}

/**
 * Areas overlap: the next one under the point after `from`, going down the stack and round to the
 * top; -1 when there is none.
 */
export function nextUnder(boxes: readonly AreaBounds[], from: number, [x, y]: SharePoint): number {
  const count = boxes.length;
  for (let step = 1; step < count; step++) {
    const j = (from - step + count) % count;
    const [left, top, right, bottom] = boxes[j];
    if (x >= left && x <= right && y >= top && y <= bottom) return j;
  }
  return -1;
}

/**
 * The area a drag on the empty video draws, kept on screen; null for a drag so short it is a click.
 */
export function drawnArea(
  [x0, y0]: SharePoint,
  [x1, y1]: SharePoint,
  size: ScreenSize,
): AreaRect | null {
  if (Math.abs(x1 - x0) * size.width < CLICK_PX && Math.abs(y1 - y0) * size.height < CLICK_PX) {
    return null;
  }
  const clampShare = (share: number) => clamp(share, 0, 1);
  return [
    clampShare(Math.min(x0, x1)),
    clampShare(Math.min(y0, y1)),
    clampShare(Math.max(x0, x1)),
    clampShare(Math.max(y0, y1)),
  ];
}

/** Whether two lists of areas cover the same parts of the frame (their kinds aside). */
export function sameAreas(a: readonly AreaBounds[], b: readonly AreaBounds[]): boolean {
  const near = (one: AreaBounds, other: AreaBounds) =>
    [0, 1, 2, 3].every((edge) => Math.abs(Number(one[edge]) - Number(other[edge])) < SAME_SHARE);
  return a.length === b.length && a.every((box, i) => near(box, b[i]));
}

/**
 * How the areas are drawn: their colors, lines and label font, from the areas' tokens
 * (themes/areas.scss).
 */
export interface AreaStyle {
  /** An area's fill (--area-fill). */
  fill: string;
  /** The selected area's fill (--area-fill-selected). */
  fillSelected: string;
  /** An area's outline color (--area-line). */
  line: string;
  /** The selected area's outline color (--area-line-selected). */
  lineSelected: string;
  /** An area's outline width, in pixels (--area-line-width). */
  lineWidth: number;
  /** The selected area's outline width, in pixels (--area-line-width-selected). */
  lineWidthSelected: number;
  /** The kind labels' font (--area-label-font). */
  labelFont: string;
  /** The background behind a label on an area that is not selected (--area-label-bg). */
  labelBg: string;
  /** A label's text color (--area-label-text). */
  labelText: string;
  /** The selected area's label color (--area-label-text-selected). */
  labelTextSelected: string;
}

/** The areas' drawing style, read from the CSS variables in effect on an element. */
export function readAreaStyle(element: Element): AreaStyle {
  const css = getComputedStyle(element);
  const token = (name: string) => css.getPropertyValue(name).trim();
  return {
    fill: token('--area-fill'),
    fillSelected: token('--area-fill-selected'),
    line: token('--area-line'),
    lineSelected: token('--area-line-selected'),
    lineWidth: Number(token('--area-line-width')),
    lineWidthSelected: Number(token('--area-line-width-selected')),
    labelFont: token('--area-label-font'),
    labelBg: token('--area-label-bg'),
    labelText: token('--area-label-text'),
    labelTextSelected: token('--area-label-text-selected'),
  };
}

/**
 * What the editor draws: the areas, the selected one, the one being drawn, and the one whose label
 * is hidden.
 */
export interface AreaScene {
  /** The areas on screen, as shares of the frame, each with its kind's id. */
  boxes: readonly AreaBox[];
  /** The selected area's index; -1 for none. */
  selected: number;
  /** The area the pointer is drawing, or null. */
  drawing: AreaRect | null;
  /** The area being dragged: its label is hidden while it moves (-1 for none). */
  moving: number;
  /** A kind's name from its id, for the labels. */
  kindName: (id: string) => string;
}

/** A label's inset from the area's corner, in pixels. */
const LABEL_INSET = 2;
/** The space either side of a label's text, in pixels. */
const LABEL_PAD = 4;
/** A label's height, in pixels. */
const LABEL_HEIGHT = 16;

/**
 * The areas over the video, each with its kind's name: plain to read on the others, see-through on
 * the selected one (its corner must stay visible), hidden while it is dragged. The area being drawn
 * has no name yet.
 */
export function drawAreas(
  context: CanvasRenderingContext2D,
  scene: AreaScene,
  size: ScreenSize,
  style: AreaStyle,
): void {
  const all: AreaBox[] = [...scene.boxes];
  if (scene.drawing) all.push([...scene.drawing, '']);
  all.forEach(([x0, y0, x1, y1, kind], i) => {
    const x = x0 * size.width;
    const y = y0 * size.height;
    const widthPx = (x1 - x0) * size.width;
    const heightPx = (y1 - y0) * size.height;
    const selected = i === scene.selected;
    context.fillStyle = selected ? style.fillSelected : style.fill;
    context.fillRect(x, y, widthPx, heightPx);
    context.strokeStyle = selected ? style.lineSelected : style.line;
    context.lineWidth = selected ? style.lineWidthSelected : style.lineWidth;
    context.strokeRect(x + 0.5, y + 0.5, widthPx - 1, heightPx - 1);
    if (!kind || i === scene.moving) return;
    const name = scene.kindName(kind);
    context.font = style.labelFont;
    context.textBaseline = 'middle';
    context.save();
    context.beginPath();
    context.rect(x + LABEL_INSET, y + LABEL_INSET, widthPx - 2 * LABEL_INSET, LABEL_HEIGHT);
    context.clip();
    if (!selected) {
      context.fillStyle = style.labelBg;
      context.fillRect(
        x + LABEL_INSET,
        y + LABEL_INSET,
        context.measureText(name).width + 2 * LABEL_PAD,
        LABEL_HEIGHT,
      );
    }
    context.fillStyle = selected ? style.labelTextSelected : style.labelText;
    context.fillText(name, x + LABEL_INSET + LABEL_PAD, y + LABEL_INSET + LABEL_HEIGHT / 2);
    context.restore();
  });
}
