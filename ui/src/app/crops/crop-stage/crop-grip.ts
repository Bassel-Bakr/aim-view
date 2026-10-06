import { CropBox, CropEntry, Shape } from '../../api';
import {
  corners,
  CropPoint,
  faced,
  faceHandle,
  flatSides,
  MIN_SIDE_PX,
  moved,
  onShape,
  pushedFlatSide,
  resized,
  turned,
  turnHandle,
} from '../../shapes/shape-geometry';
import {
  pushedSide,
  solidCorners,
  SideHandle,
  sideHandles,
  tumbled,
  tumbleHandle,
} from '../../shapes/solid-geometry';
import { freePoints, freeSides, movedSide, movedVertex } from '../../shapes/free-geometry';
import { changeEach, DraftScene, withChanged } from '../crop-scene';

/**
 * What a finger takes hold of on the Crops page's stage, and what dragging it does to the scene. Points are crop
 * pixels; `scale` is screen pixels per crop pixel, so handles keep their size on screen at any zoom.
 */

/**
 * What a press holds: a selected shape's corner, side, turn handle, face handle or a solid's tumble handle, a shape, a
 * crossed-out box, or none.
 */
export type GripKind = 'corner' | 'side' | 'turn' | 'face' | 'tumble' | 'move' | 'uncross' | 'draw';

export interface CropGrip {
  kind: GripKind;
  /** The shape held; null for a crossed-out box or the wall. */
  id: string | null;
  /** The corner held (0 to 3, as `corners`), the side (as its handle says), or the crossed-out model box; -1: none. */
  index: number;
}

/**
 * A drag: where it began and where it is (crop pixels), whether Shift keeps a shape's two sides equal, and whether a
 * side's opposite side moves with it (Alt, or the tools' Mirror), the shape keeping its middle.
 */
export interface CropDrag {
  start: CropPoint;
  point: CropPoint;
  even: boolean;
  mirror: boolean;
}

/** A selected shape's handles: its corners (a solid box's front ones), its sides, turn and face handles, a solid's tumble. */
export interface ShapeHandles {
  corners: CropPoint[];
  sides: SideHandle[];
  turn: CropPoint;
  face: CropPoint | null;
  tumble: CropPoint | null;
}

/** How far the turn and face handles sit from a shape, in screen pixels. */
const HANDLE_REACH_PX = 22;
/** How near a mouse must come to a handle to take it, in screen pixels; a finger, which covers more, TOUCH_HIT_PX. */
const HANDLE_HIT_PX = 16;
const TOUCH_HIT_PX = 30;

/** How near a pointer of a type ('mouse', 'pen', 'touch') must come to a handle to take it, in screen pixels. */
export function handleReach(pointerType: string): number {
  return pointerType === 'touch' ? TOUCH_HIT_PX : HANDLE_HIT_PX;
}
/** How near a finger must come to a shape's edge to take it, in screen pixels. */
const SHAPE_SLACK_PX = 6;

/** A shape's handles at a scale: its corners, its sides (a solid box's every face, a capsule's ends and sides). */
export function handlesOf(shape: Shape, scale: number): ShapeHandles {
  const reach = HANDLE_REACH_PX / scale;
  const solid = shape.solid;
  const points = freePoints(shape);
  if (points) {
    return {
      corners: points,
      sides: freeSides(points),
      turn: turnHandle(shape, reach),
      face: null,
      tumble: null,
    };
  }
  return {
    corners: vertexHandles(shape),
    sides: solid ? sideHandles(shape, solid) : flatSides(shape),
    turn: turnHandle(shape, reach),
    face: faceHandle(shape),
    tumble: solid && tumbleHandle(shape, solid, reach),
  };
}

/** A shape's corner handles: a box's every vertex (a 3D box's eight), a pill's frame corners, a capsule's none. */
function vertexHandles(shape: Shape): CropPoint[] {
  if (shape.kind === 'box') return shape.solid ? solidCorners(shape, shape.solid) : corners(shape);
  return shape.solid ? [] : corners(shape);
}

/**
 * A box's vertex dragged: it moves on its own, the others stay (a box not yet placed by hand takes its corners as its
 * placed vertices first). A pill's corner resizes its frame, the opposite corner staying (Shift: equal sides).
 */
function draggedVertex(shape: Shape, index: number, point: CropPoint, even: boolean): Shape {
  if (shape.kind !== 'box') return resized(shape, index, point, even);
  const points = freePoints(shape) ?? vertexHandles(shape);
  return movedVertex(shape, points, index, point);
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

/** The handle of the selected shape a press takes: corners, then the sides the camera sees, then the rest. */
function handleAt(
  shape: Shape,
  near: (handle: CropPoint | null) => boolean,
  scale: number,
): CropGrip | null {
  const handles = handlesOf(shape, scale);
  const corner = handles.corners.findIndex(near);
  if (corner >= 0) return { kind: 'corner', id: shape.id, index: corner };
  // the sides the camera sees first: they lie over the hidden ones
  const sides = [...handles.sides].sort((a, b) => Number(b.seen) - Number(a.seen));
  const side = sides.find((handle) => near(handle.point));
  if (side) return { kind: 'side', id: shape.id, index: side.side };
  if (near(handles.turn)) return { kind: 'turn', id: shape.id, index: -1 };
  if (near(handles.tumble)) return { kind: 'tumble', id: shape.id, index: -1 };
  if (near(handles.face)) return { kind: 'face', id: shape.id, index: -1 };
  return null;
}

/** What a press at a point takes: the selected shape's handle first, then the topmost shape, then a crossed-out box. */
export function gripAt(
  scene: DraftScene,
  crop: CropEntry,
  selection: readonly string[],
  point: CropPoint,
  scale: number,
  reachPx = HANDLE_HIT_PX,
): CropGrip {
  const near = (handle: CropPoint | null) =>
    handle !== null && Math.hypot(handle[0] - point[0], handle[1] - point[1]) * scale <= reachPx;
  const shape = handled(scene, selection);
  const handle = shape && handleAt(shape, near, scale);
  if (handle) return handle;
  const [top] = shapesAt(scene, point, scale);
  if (top) return { kind: 'move', id: top.id, index: -1 };
  const crossed = crossedAt(scene, crop, point, scale);
  return crossed >= 0
    ? { kind: 'uncross', id: null, index: crossed }
    : { kind: 'draw', id: null, index: -1 };
}

/**
 * The scene as a drag leaves it, from the scene at the press: a corner resizes its shape (its sides kept equal with
 * Shift), a side moves that side (its opposite one too, mirrored), the turn handle turns it, the face handle moves its
 * far end, a solid's tumble handle tumbles it in 3D, and a shape moves (with the rest of the selection when selected).
 */
export function dragged(
  from: DraftScene,
  grip: CropGrip,
  selection: readonly string[],
  { start, point, even, mirror }: CropDrag,
): DraftScene {
  const held = from.shapes.find((shape) => shape.id === grip.id);
  if (!held) return from;
  const solid = held.solid;
  switch (grip.kind) {
    case 'corner':
      return withChanged(from, draggedVertex(held, grip.index, point, even));
    case 'side':
      return withChanged(from, draggedSide(held, grip.index, { start, point, even, mirror }));
    case 'turn':
      return withChanged(from, turned(held, point));
    case 'face':
      return withChanged(from, faced(held, point));
    case 'tumble':
      return solid
        ? withChanged(from, tumbled(held, solid, [point[0] - start[0], point[1] - start[1]]))
        : from;
    case 'move': {
      const ids = selection.includes(held.id) ? selection : [held.id];
      return changeEach(from, ids, (shape) =>
        moved(shape, [point[0] - start[0], point[1] - start[1]]),
      );
    }
    default:
      return from;
  }
}

/** A side dragged: a placed box's moves its vertices, a solid's its face, a flat shape's its frame's side. */
function draggedSide(shape: Shape, side: number, { start, point, mirror }: CropDrag): Shape {
  const points = freePoints(shape);
  if (points)
    return movedSide(shape, points, side, [point[0] - start[0], point[1] - start[1]], mirror);
  if (shape.solid)
    return pushedSide(shape, shape.solid, side, point, { minSide: MIN_SIDE_PX, mirror });
  return pushedFlatSide(shape, side, point, mirror);
}

/**
 * The box a drag on the wall draws, to a tenth of a pixel; null when it is too small to be a shape. With Shift (`even`)
 * both sides take the longer one, from where the drag began toward the pointer: a perfect circle or square.
 */
export function sketchBox({ start: [x0, y0], point: [x1, y1], even }: CropDrag): CropBox | null {
  const side = Math.max(Math.abs(x1 - x0), Math.abs(y1 - y0));
  const [endX, endY] = even
    ? [x0 + Math.sign(x1 - x0) * side, y0 + Math.sign(y1 - y0) * side]
    : [x1, y1];
  const [width, height] = [Math.abs(endX - x0), Math.abs(endY - y0)];
  if (width < MIN_SIDE_PX || height < MIN_SIDE_PX) return null;
  const tenth = (value: number) => Math.round(value * 10) / 10;
  return [tenth((x0 + endX) / 2), tenth((y0 + endY) / 2), tenth(width), tenth(height)];
}
