import { CropBox, CropEntry, Shape } from '../../api';
import {
  corners,
  CropPoint,
  faced,
  faceHandle,
  MIN_SIDE_PX,
  moved,
  onShape,
  resized,
  turned,
  turnHandle,
} from '../../shapes/shape-geometry';
import { changeEach, DraftScene, withChanged } from '../crop-scene';

/**
 * What a finger takes hold of on the Crops page's stage, and what dragging it does to the scene. Points are crop
 * pixels; `scale` is screen pixels per crop pixel, so handles keep their size on screen at any zoom.
 */

/** What a press holds: a selected shape's corner, turn handle or face handle, a shape, a crossed-out box, or none. */
export type GripKind = 'corner' | 'turn' | 'face' | 'move' | 'uncross' | 'draw';

export interface CropGrip {
  kind: GripKind;
  /** The shape held; null for a crossed-out box or the wall. */
  id: string | null;
  /** The corner held (0 to 3, as `corners`), or the crossed-out model box; -1 for none. */
  index: number;
}

/** A selected shape's handles: its corners, its turn handle, and a box's face handle. */
export interface ShapeHandles {
  corners: CropPoint[];
  turn: CropPoint;
  face: CropPoint | null;
}

/** How far the turn and face handles sit from a shape, in screen pixels. */
const HANDLE_REACH_PX = 22;
/** How near a finger must come to a handle to take it, in screen pixels. */
const HANDLE_HIT_PX = 16;
/** How near a finger must come to a shape's edge to take it, in screen pixels. */
const SHAPE_SLACK_PX = 6;

/** A shape's handles at a scale. */
export function handlesOf(shape: Shape, scale: number): ShapeHandles {
  const reach = HANDLE_REACH_PX / scale;
  return {
    corners: corners(shape),
    turn: turnHandle(shape, reach),
    face: faceHandle(shape, reach),
  };
}

/** The shape that shows handles: the selected one, when only one is (with more, a drag moves them all). */
export function handled(scene: DraftScene, selection: readonly string[]): Shape | null {
  return selection.length === 1
    ? (scene.shapes.find((shape) => shape.id === selection[0]) ?? null)
    : null;
}

/** The shapes under a point, the topmost first (the nearest, then the one drawn last). */
export function shapesAt(scene: DraftScene, point: CropPoint, scale: number): Shape[] {
  const slack = SHAPE_SLACK_PX / scale;
  return scene.shapes
    .map((shape, order) => ({ shape, order }))
    .filter(({ shape }) => onShape(shape, point, slack))
    .sort((a, b) => b.shape.depth - a.shape.depth || b.order - a.order)
    .map(({ shape }) => shape);
}

/** The crossed-out model box under a point; -1 for none. */
function crossedAt(scene: DraftScene, crop: CropEntry, [x, y]: CropPoint, scale: number): number {
  const slack = SHAPE_SLACK_PX / scale;
  const under = scene.crossed.find((model) => {
    const box = crop.boxes[model];
    return (
      box &&
      Math.abs(x - box[0]) <= box[2] / 2 + slack &&
      Math.abs(y - box[1]) <= box[3] / 2 + slack
    );
  });
  return under ?? -1;
}

/** What a press at a point takes: the selected shape's handle first, then the topmost shape, then a crossed-out box. */
export function gripAt(
  scene: DraftScene,
  crop: CropEntry,
  selection: readonly string[],
  point: CropPoint,
  scale: number,
): CropGrip {
  const near = (handle: CropPoint | null) =>
    handle !== null &&
    Math.hypot(handle[0] - point[0], handle[1] - point[1]) * scale <= HANDLE_HIT_PX;
  const shape = handled(scene, selection);
  if (shape) {
    const handles = handlesOf(shape, scale);
    const corner = handles.corners.findIndex(near);
    if (corner >= 0) return { kind: 'corner', id: shape.id, index: corner };
    if (near(handles.turn)) return { kind: 'turn', id: shape.id, index: -1 };
    if (near(handles.face)) return { kind: 'face', id: shape.id, index: -1 };
  }
  const [top] = shapesAt(scene, point, scale);
  if (top) return { kind: 'move', id: top.id, index: -1 };
  const crossed = crossedAt(scene, crop, point, scale);
  return crossed >= 0
    ? { kind: 'uncross', id: null, index: crossed }
    : { kind: 'draw', id: null, index: -1 };
}

/**
 * The scene as a drag leaves it, from the scene at the press: a corner resizes its shape, the turn handle turns it, the
 * face handle gives a box its third face, and a shape moves (with the rest of the selection when it is selected).
 */
export function dragged(
  from: DraftScene,
  grip: CropGrip,
  selection: readonly string[],
  start: CropPoint,
  point: CropPoint,
): DraftScene {
  const held = from.shapes.find((shape) => shape.id === grip.id);
  if (!held) return from;
  if (grip.kind === 'corner') return withChanged(from, resized(held, grip.index, point));
  if (grip.kind === 'turn') return withChanged(from, turned(held, point));
  if (grip.kind === 'face') return withChanged(from, faced(held, point));
  if (grip.kind !== 'move') return from;
  const ids = selection.includes(held.id) ? selection : [held.id];
  return changeEach(from, ids, (shape) => moved(shape, [point[0] - start[0], point[1] - start[1]]));
}

/** The box a drag on the wall draws, to a tenth of a pixel; null when it is too small to be a shape. */
export function sketchBox([x0, y0]: CropPoint, [x1, y1]: CropPoint): CropBox | null {
  const [width, height] = [Math.abs(x1 - x0), Math.abs(y1 - y0)];
  if (width < MIN_SIDE_PX || height < MIN_SIDE_PX) return null;
  const tenth = (value: number) => Math.round(value * 10) / 10;
  return [tenth((x0 + x1) / 2), tenth((y0 + y1) / 2), tenth(width), tenth(height)];
}
