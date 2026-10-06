import { Shape, Solid } from '../api';
import type { CropPoint } from './shape-geometry';

/**
 * A solid (3D) shape on a crop, as the core draws it (src/shapes.rs `rotation`, `solid_corners`,
 * `solid_pill_segment`): a box or a capsule with a thickness, tipped and swung out of the screen's plane, seen
 * straight on from the camera. Here: where its corners and axis fall, its handles (turn, tumble, thickness), what a
 * drag on them does, and its wireframe. Coordinates are crop pixels; angles are degrees; x runs to the right, y down,
 * z away from the camera.
 */

/** A point or a direction in 3D, crop pixels. */
type Vector3 = [x: number, y: number, z: number];
/** A 3D turn as the rows of its matrix. */
type Turn = [Vector3, Vector3, Vector3];
/** A box edge: the indexes of its two corners in `solidCorners`. */
export type Edge = [from: number, to: number];

/** How a new 3D shape starts: tipped and swung so its top and right side show (a target seen from above, to the right). */
export const START_TIP_DEG = 20;
export const START_SWING_DEG = 25;
/** How far a drag across half a shape's size tumbles it, in degrees. */
const TUMBLE_DEG_PER_HALF = 90;
/** Points round each end of a capsule's outline. */
const RING_POINTS = 32;

const radians = (degrees: number) => (degrees * Math.PI) / 180;
const degreesOf = (turn: number) => (turn * 180) / Math.PI;

/** Rz(angle) Ry(swing) Rx(tip), as the core's `rotation`. */
export function rotation(shape: Shape, solid: Solid): Turn {
  const [sinZ, cosZ] = [Math.sin(radians(shape.angle)), Math.cos(radians(shape.angle))];
  const [sinY, cosY] = [Math.sin(radians(solid.swing)), Math.cos(radians(solid.swing))];
  const [sinX, cosX] = [Math.sin(radians(solid.tip)), Math.cos(radians(solid.tip))];
  return [
    [cosZ * cosY, cosZ * sinY * sinX - sinZ * cosX, cosZ * sinY * cosX + sinZ * sinX],
    [sinZ * cosY, sinZ * sinY * sinX + cosZ * cosX, sinZ * sinY * cosX - cosZ * sinX],
    [-sinY, cosY * sinX, cosY * cosX],
  ];
}

function times(turn: Turn, vector: Vector3): Vector3 {
  return turn.map((row) => row[0] * vector[0] + row[1] * vector[1] + row[2] * vector[2]) as Vector3;
}

function product(a: Turn, b: Turn): Turn {
  return a.map((row) =>
    [0, 1, 2].map(
      (column) => row[0] * b[0][column] + row[1] * b[1][column] + row[2] * b[2][column],
    ),
  ) as Turn;
}

/** A point of a solid shape's own frame (along its width, across it, front to back) on the crop. */
function onCrop(shape: Shape, turn: Turn, own: Vector3): CropPoint {
  const [x, y] = times(turn, own);
  return [shape.box[0] + x, shape.box[1] + y];
}

/** A solid box's eight corners, as the core's: bit 1 of the index its right side, bit 2 its bottom, bit 4 its back. */
export function solidCorners(shape: Shape, solid: Solid): CropPoint[] {
  const turn = rotation(shape, solid);
  const half = [shape.box[2] / 2, shape.box[3] / 2, solid.thickness / 2];
  return [0, 1, 2, 3, 4, 5, 6, 7].map((corner) =>
    onCrop(shape, turn, half.map((size, axis) => ((corner >> axis) & 1 ? size : -size)) as Vector3),
  );
}

/** Its front face's corners, in the order of a flat shape's corners (shape-geometry `corners`). */
export function frontCorners(shape: Shape, solid: Solid): CropPoint[] {
  const all = solidCorners(shape, solid);
  return [all[0], all[1], all[3], all[2]];
}

/** A capsule's radius: half its short side. */
function radiusOf(shape: Shape): number {
  return Math.min(shape.box[2], shape.box[3]) / 2;
}

/** A capsule's axis in its own frame, a unit along its long side. */
function axisOf(shape: Shape): Vector3 {
  return shape.box[2] >= shape.box[3] ? [1, 0, 0] : [0, 1, 0];
}

/** A solid pill's axis ends on the crop: its middle segment, tipped toward or away from the camera. */
export function pillAxis(shape: Shape, solid: Solid): CropPoint[] {
  const half = Math.max(shape.box[2], shape.box[3]) / 2 - radiusOf(shape);
  const turn = rotation(shape, solid);
  const unit = axisOf(shape);
  return [-half, half].map((along) =>
    onCrop(shape, turn, unit.map((part) => part * along) as Vector3),
  );
}

export const EDGES: Edge[] = [0, 1, 2, 3, 4, 5, 6, 7].flatMap((corner) =>
  [1, 2, 4].filter((side) => !(corner & side)).map((side): Edge => [corner, corner | side]),
);

export function segmentDistance(
  [x, y]: CropPoint,
  [ax, ay]: CropPoint,
  [bx, by]: CropPoint,
): number {
  const [alongX, alongY] = [bx - ax, by - ay];
  const lengthSq = alongX * alongX + alongY * alongY;
  const share =
    lengthSq > 0 ? Math.min(Math.max(((x - ax) * alongX + (y - ay) * alongY) / lengthSq, 0), 1) : 0;
  return Math.hypot(x - ax - share * alongX, y - ay - share * alongY);
}

/** The convex hull of points, in order round it (Andrew's monotone chain, as the core's). */
export function convexHull(points: CropPoint[]): CropPoint[] {
  const sorted = [...points].sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const turn = (from: CropPoint, a: CropPoint, b: CropPoint) =>
    (a[0] - from[0]) * (b[1] - from[1]) - (a[1] - from[1]) * (b[0] - from[0]);
  const hull: CropPoint[] = [];
  for (const pass of [sorted, [...sorted].reverse()]) {
    const start = hull.length;
    for (const point of pass) {
      while (
        hull.length >= start + 2 &&
        turn(hull[hull.length - 2], hull[hull.length - 1], point) <= 0
      ) {
        hull.pop();
      }
      hull.push(point);
    }
    hull.pop();
  }
  return hull;
}

export function inConvex(polygon: CropPoint[], [x, y]: CropPoint): boolean {
  const sides = polygon.map(([ax, ay], i) => {
    const [bx, by] = polygon[(i + 1) % polygon.length];
    return Math.sign((bx - ax) * (y - ay) - (by - ay) * (x - ax));
  });
  return (
    polygon.length >= 3 && (sides.every((side) => side >= 0) || sides.every((side) => side <= 0))
  );
}

/** Whether a point is on a solid shape, `slack` pixels round it counting too. */
export function onSolid(shape: Shape, solid: Solid, point: CropPoint, slack: number): boolean {
  if (shape.kind === 'pill') {
    const [a, b] = pillAxis(shape, solid);
    return segmentDistance(point, a, b) <= radiusOf(shape) + slack;
  }
  const hull = convexHull(solidCorners(shape, solid));
  return (
    inConvex(hull, point) ||
    hull.some((a, i) => segmentDistance(point, a, hull[(i + 1) % hull.length]) <= slack)
  );
}

/** The points a box round a solid shape must take in: a box's corners, a capsule's ends widened by its radius. */
export function solidExtent(shape: Shape, solid: Solid): CropPoint[] {
  if (shape.kind === 'box') return solidCorners(shape, solid);
  const radius = radiusOf(shape);
  return pillAxis(shape, solid).flatMap(([x, y]): CropPoint[] => [
    [x - radius, y - radius],
    [x + radius, y + radius],
  ]);
}

/** How far a solid shape reaches from its middle: its farthest extent point. */
function reachOf(shape: Shape, solid: Solid): number {
  const [cx, cy] = shape.box;
  return Math.max(...solidExtent(shape, solid).map(([x, y]) => Math.hypot(x - cx, y - cy)));
}

/** Where a solid shape's turn handle is: beyond it, `reach` pixels off, in the direction of its angle (up at 0). */
export function solidTurnHandle(shape: Shape, solid: Solid, reach: number): CropPoint {
  const [cx, cy] = shape.box;
  const out = reachOf(shape, solid) + reach;
  return [cx + out * Math.sin(radians(shape.angle)), cy - out * Math.cos(radians(shape.angle))];
}

/** Where its tumble handle is: opposite the turn handle, below it at angle 0. */
export function tumbleHandle(shape: Shape, solid: Solid, reach: number): CropPoint {
  const [cx, cy] = shape.box;
  const [x, y] = solidTurnHandle(shape, solid, reach);
  return [2 * cx - x, 2 * cy - y];
}

/** A turn of some radians about an axis (a unit vector), Rodrigues' formula. */
function axisTurn([ax, ay, az]: Vector3, turn: number): Turn {
  const [sin, cos] = [Math.sin(turn), Math.cos(turn)];
  const rest = 1 - cos;
  return [
    [cos + ax * ax * rest, ax * ay * rest - az * sin, ax * az * rest + ay * sin],
    [ay * ax * rest + az * sin, cos + ay * ay * rest, ay * az * rest - ax * sin],
    [az * ax * rest - ay * sin, az * ay * rest + ax * sin, cos + az * az * rest],
  ];
}

/** A turn read back as Rz(angle) Ry(swing) Rx(tip): [angle, swing, tip], degrees. */
function anglesOf(turn: Turn): Vector3 {
  const swing = Math.asin(Math.min(Math.max(-turn[2][0], -1), 1));
  if (Math.abs(Math.cos(swing)) < 1e-9) {
    return [degreesOf(Math.atan2(-turn[0][1], turn[1][1])), degreesOf(swing), 0];
  }
  return [
    degreesOf(Math.atan2(turn[1][0], turn[0][0])),
    degreesOf(swing),
    degreesOf(Math.atan2(turn[2][1], turn[2][2])),
  ];
}

/**
 * A solid shape tumbled by a drag ([dx, dy], crop pixels, from where it was at the press), as a ball rolled under a
 * finger: its front follows the drag, about the axis across it; half its size turns it TUMBLE_DEG_PER_HALF.
 */
export function tumbled(shape: Shape, solid: Solid, [dx, dy]: CropPoint): Shape {
  const length = Math.hypot(dx, dy);
  if (length === 0) return shape;
  const half = Math.max(shape.box[2], shape.box[3], solid.thickness) / 2;
  return turnedInSpace(
    shape,
    solid,
    [dy / length, -dx / length, 0],
    (length / half) * TUMBLE_DEG_PER_HALF,
  );
}

/** A solid shape turned about an axis of the screen (a unit vector: x to the right, y down, z away), in degrees. */
export function turnedInSpace(shape: Shape, solid: Solid, axis: Vector3, degrees: number): Shape {
  const [angle, swing, tip] = anglesOf(
    product(axisTurn(axis, radians(degrees)), rotation(shape, solid)),
  );
  return { ...shape, angle: (angle + 360) % 360, solid: { ...solid, tip, swing } };
}

/** Where a turn button turns a solid: one of its sides toward the camera. */
export type SideShown = 'top' | 'bottom' | 'left' | 'right';

const SHOWING: Readonly<Record<SideShown, Vector3>> = {
  top: [1, 0, 0],
  bottom: [-1, 0, 0],
  right: [0, 1, 0],
  left: [0, -1, 0],
};

/** A solid shape turned so a side comes toward the camera by some degrees, about the screen's own axes. */
export function showing(shape: Shape, solid: Solid, side: SideShown, degrees: number): Shape {
  return turnedInSpace(shape, solid, SHOWING[side], degrees);
}

/** A face of a solid box on the crop: its corners in order round it, whether it faces the camera, and its light (0 to 1). */
export interface SolidFace {
  corners: CropPoint[];
  facing: boolean;
  light: number;
}

/** An edge of a solid box on the crop, and whether the camera sees it (a face it bounds faces the camera). */
export interface SolidEdge {
  from: CropPoint;
  to: CropPoint;
  seen: boolean;
}

/** Where the light comes from: above, to the left and in front (a unit vector toward it). */
const LIGHT: Vector3 = [-0.4, -0.7, -0.6].map(
  (part) => part / Math.hypot(0.4, 0.7, 0.6),
) as Vector3;

/** A box face's outward direction, after the turn: its axis's column of the matrix, toward its side. */
function faceNormal(turn: Turn, face: number): Vector3 {
  const [axis, side] = [face >> 1, face & 1 ? 1 : -1];
  return turn.map((row) => row[axis] * side) as Vector3;
}

/** The corners of a box face (axis * 2, plus 1 for its far side), as `solidCorners` indexes in order round it. */
export function faceCornerIndexes(face: number): number[] {
  const axis = face >> 1;
  const [first, second] = [0, 1, 2].filter((other) => other !== axis);
  const base = (face & 1) << axis;
  return [
    [0, 0],
    [1, 0],
    [1, 1],
    [0, 1],
  ].map(([a, b]) => base | (a << first) | (b << second));
}

/** A solid box's six faces: index axis * 2 (left, top, front) and axis * 2 + 1 (right, bottom, back). */
export function solidFaces(shape: Shape, solid: Solid): SolidFace[] {
  const turn = rotation(shape, solid);
  const points = solidCorners(shape, solid);
  return [0, 1, 2, 3, 4, 5].map((face) => {
    const normal = faceNormal(turn, face);
    const light = normal[0] * LIGHT[0] + normal[1] * LIGHT[1] + normal[2] * LIGHT[2];
    return {
      corners: faceCornerIndexes(face).map((corner) => points[corner]),
      facing: normal[2] < 0,
      light: Math.max(0, light),
    };
  });
}

/** A solid box's twelve edges, each seen when a face it bounds faces the camera. */
export function solidEdges(shape: Shape, solid: Solid): SolidEdge[] {
  const faces = solidFaces(shape, solid);
  const points = solidCorners(shape, solid);
  return EDGES.map(([from, to]) => {
    const bounds = faces.filter((_face, index) => {
      const corners = faceCornerIndexes(index);
      return corners.includes(from) && corners.includes(to);
    });
    return { from: points[from], to: points[to], seen: bounds.some((face) => face.facing) };
  });
}

/** One end ring of a capsule on the crop: an ellipse, and whether it is the end nearer the camera. */
export interface CapsuleRing {
  center: CropPoint;
  /** Half the ring along the axis on screen (shorter the more the axis faces the camera) and across it. */
  along: number;
  across: number;
  /** The axis's direction on screen, radians. */
  tilt: number;
  near: boolean;
}

/** A capsule's outline on the crop (its ends' circles and the lines joining them), in order round it. */
export function capsuleOutline(shape: Shape, solid: Solid): CropPoint[] {
  const radius = radiusOf(shape);
  const ring = (center: CropPoint) =>
    [...Array(RING_POINTS).keys()].map((step): CropPoint => {
      const turn = (2 * Math.PI * step) / RING_POINTS;
      return [center[0] + radius * Math.cos(turn), center[1] + radius * Math.sin(turn)];
    });
  return convexHull(pillAxis(shape, solid).flatMap(ring));
}

/** A capsule's two end rings, where its ends join its middle. */
export function capsuleRings(shape: Shape, solid: Solid): CapsuleRing[] {
  const radius = radiusOf(shape);
  const [nx, ny, nz] = times(rotation(shape, solid), axisOf(shape));
  const tilt = Math.atan2(ny, nx);
  return pillAxis(shape, solid).map((center, end) => ({
    center,
    along: radius * Math.abs(nz),
    across: radius,
    tilt,
    near: (end === 0 ? -nz : nz) < 0,
  }));
}

/**
 * A solid's side handle: where it is, which side it moves (a box face: axis * 2, plus 1 for its far side; a capsule:
 * 0 and 1 its ends, 2 its thickness), and whether the camera sees that side.
 */
export interface SideHandle {
  point: CropPoint;
  side: number;
  seen: boolean;
}

/** The shortest a side handle's direction on screen may be (a side facing the camera squarely has none). */
const MIN_SIDE_REACH = 0.2;
/** A capsule's thickness handle. */
const CAPSULE_THICKNESS = 2;

/** A size of a solid shape along each of its axes: width, height, thickness. */
function sizesOf(shape: Shape, solid: Solid): Vector3 {
  return [shape.box[2], shape.box[3], solid.thickness];
}

/** A solid's side handles: a box's faces, a capsule's ends and both its sides; each where it can be dragged on screen. */
export function sideHandles(shape: Shape, solid: Solid): SideHandle[] {
  const turn = rotation(shape, solid);
  const onScreen = (own: Vector3) => onCrop(shape, turn, own);
  const reaches = (direction: Vector3) =>
    Math.hypot(...times(turn, direction).slice(0, 2)) >= MIN_SIDE_REACH;
  if (shape.kind === 'box') {
    const sizes = sizesOf(shape, solid);
    return [0, 1, 2, 3, 4, 5].flatMap((side) => {
      const [axis, sign] = [side >> 1, side & 1 ? 1 : -1];
      const unit = [0, 1, 2].map((index) => (index === axis ? sign : 0)) as Vector3;
      if (!reaches(unit)) return [];
      const point = onScreen(unit.map((part) => (part * sizes[axis]) / 2) as Vector3);
      return [{ point, side, seen: faceNormal(turn, side)[2] < 0 }];
    });
  }
  const [axis, radius] = [axisOf(shape), radiusOf(shape)];
  const tip = Math.max(shape.box[2], shape.box[3]) / 2;
  const across: Vector3 = [axis[1], axis[0], 0];
  const ends = reaches(axis)
    ? [-1, 1].map((sign, side) => ({
        point: onScreen(axis.map((part) => part * sign * tip) as Vector3),
        side,
        seen: times(turn, axis)[2] * sign <= 0,
      }))
    : [];
  const sides = [-1, 1].map((sign) => ({
    point: onScreen(across.map((part) => part * sign * radius) as Vector3),
    side: CAPSULE_THICKNESS,
    seen: true,
  }));
  return [...ends, ...sides];
}

/** How far a point lies along a direction on screen from a shape's middle, in units of that direction. */
function alongOf(shape: Shape, [x, y]: CropPoint, [dx, dy]: CropPoint): number | null {
  const lengthSq = dx * dx + dy * dy;
  if (lengthSq < MIN_SIDE_REACH * MIN_SIDE_REACH) return null;
  return ((x - shape.box[0]) * dx + (y - shape.box[1]) * dy) / lengthSq;
}

/** How a side is pushed: the smallest a side may get (crop pixels), and whether its opposite side moves with it. */
export interface SidePush {
  minSide: number;
  mirror: boolean;
}

/**
 * A solid shape with one side moved to a point, along that side's direction on screen: a box face (the opposite face
 * stays, or moves the other way with `mirror`: its size on that axis and its middle change), a capsule's end (the
 * other end stays, or mirrors it) or its thickness.
 */
export function pushedSide(
  shape: Shape,
  solid: Solid,
  side: number,
  point: CropPoint,
  { minSide, mirror }: SidePush,
): Shape {
  const turn = rotation(shape, solid);
  const sizes = sizesOf(shape, solid);
  if (shape.kind === 'pill' && side === CAPSULE_THICKNESS) {
    const across = shape.box[2] >= shape.box[3] ? 1 : 0;
    const along = alongOf(shape, point, [turn[0][across], turn[1][across]]);
    if (along === null) return shape;
    const long = Math.max(shape.box[2], shape.box[3]);
    const thick = Math.min(long, Math.max(minSide, 2 * Math.abs(along)));
    const box: Shape['box'] = [...shape.box];
    box[2 + across] = thick;
    return { ...shape, box, solid: { ...solid, thickness: thick } };
  }
  const axis = shape.kind === 'pill' ? (shape.box[2] >= shape.box[3] ? 0 : 1) : side >> 1;
  const sign = (shape.kind === 'pill' ? side : side & 1) ? 1 : -1;
  const [dx, dy] = [turn[0][axis] * sign, turn[1][axis] * sign];
  const along = alongOf(shape, point, [dx, dy]);
  if (along === null) return shape;
  const smallest = shape.kind === 'pill' ? Math.min(shape.box[2], shape.box[3]) : minSide;
  const size = Math.max(smallest, mirror ? 2 * along : along + sizes[axis] / 2);
  const grown = mirror ? 0 : (size - sizes[axis]) / 2;
  const next: Vector3 = [...sizes];
  next[axis] = size;
  return {
    ...shape,
    box: [shape.box[0] + dx * grown, shape.box[1] + dy * grown, next[0], next[1]],
    solid: { ...solid, thickness: next[2] },
  };
}

/** Each front corner's side along the width and across it, in `frontCorners` order. */
const CORNER_SIDES: CropPoint[] = [
  [-1, -1],
  [1, -1],
  [1, 1],
  [-1, 1],
];

/**
 * A solid shape resized from a front corner to a point: the opposite front corner stays, and the width and height
 * follow the front face's own axes on screen (unchanged while the face is edge on). With `even` both take the larger.
 */
export function resizedSolid(
  shape: Shape,
  solid: Solid,
  corner: number,
  [x, y]: CropPoint,
  even: boolean,
  minSide: number,
): Shape {
  const turn = rotation(shape, solid);
  const [qx, qy] = frontCorners(shape, solid)[(corner + 2) % 4];
  const [ax, ay, bx, by] = [turn[0][0], turn[1][0], turn[0][1], turn[1][1]];
  const det = ax * by - ay * bx;
  if (Math.abs(det) < 1e-6) return shape;
  const [dx, dy] = [x - qx, y - qy];
  const [sideX, sideY] = CORNER_SIDES[corner];
  let width = Math.max(minSide, (sideX * (dx * by - dy * bx)) / det);
  let height = Math.max(minSide, (sideY * (ax * dy - ay * dx)) / det);
  if (even) width = height = Math.max(width, height);
  const [frontX, frontY] = [
    (-turn[0][2] * solid.thickness) / 2,
    (-turn[1][2] * solid.thickness) / 2,
  ];
  const cx = qx + (ax * sideX * width) / 2 + (bx * sideY * height) / 2 - frontX;
  const cy = qy + (ay * sideX * width) / 2 + (by * sideY * height) / 2 - frontY;
  return { ...shape, box: [cx, cy, width, height] };
}

/**
 * A solid shape's wireframe as a path, the context drawing in crop pixels: a box's twelve edges; a capsule's outline
 * and the rings where its ends join its middle (flattened as its axis tips toward the camera).
 */
export function traceSolid(context: CanvasRenderingContext2D, shape: Shape, solid: Solid): void {
  if (shape.kind === 'box') {
    const points = solidCorners(shape, solid);
    for (const [from, to] of EDGES) {
      context.moveTo(...points[from]);
      context.lineTo(...points[to]);
    }
    return;
  }
  const radius = radiusOf(shape);
  const ends = pillAxis(shape, solid);
  const ring = (center: CropPoint) =>
    [...Array(RING_POINTS).keys()].map((step): CropPoint => {
      const turn = (2 * Math.PI * step) / RING_POINTS;
      return [center[0] + radius * Math.cos(turn), center[1] + radius * Math.sin(turn)];
    });
  const outline = convexHull(ends.flatMap(ring));
  outline.forEach((point, i) => (i ? context.lineTo(...point) : context.moveTo(...point)));
  context.closePath();
  const [nx, ny, nz] = times(rotation(shape, solid), axisOf(shape));
  const tilt = Math.atan2(ny, nx);
  const across = radius * Math.abs(nz);
  for (const [x, y] of ends) {
    context.moveTo(x + across * Math.cos(tilt), y + across * Math.sin(tilt));
    context.ellipse(x, y, across, radius, tilt, 0, 2 * Math.PI);
  }
}
