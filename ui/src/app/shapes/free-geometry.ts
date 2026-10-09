/**
 * A box whose vertices are placed by hand (Shape.points: a flat box's 4 corners in order round
 * it, from its top left; a 3D box's 8 in `solidCorners` order), for a target seen in perspective;
 * or a polygon's, in order round it (moved, turned and scaled the same way, each vertex dragged).
 * Each vertex drags on its own, a side (a 3D box's face) moves its vertices together, and the
 * outline is what they span, as the core's (src/shapes.rs `outline`). Crop pixels. Out:
 * shape-geometry.ts and the Crops page's stage.
 */

import { CropBox, Shape } from '../api';
import type { CropPoint } from './shape-geometry';
import {
  convexHull,
  EDGES,
  faceCornerIndexes,
  inConvex,
  segmentDistance,
  SideHandle,
  SolidEdge,
  SolidFace,
  solidCorners,
  solidFaces,
} from './solid-geometry';

/** A flat box's sides as its corners' indexes: left, right, top, bottom (as shape-geometry's `flatSides`). */
const FLAT_SIDES: number[][] = [
  [3, 0],
  [1, 2],
  [0, 1],
  [2, 3],
];
/** A 3D box's vertex count. */
const SOLID_POINTS = 8;

/** Each 3D face's winding on screen when it faces the camera: +1 or -1, from a box seen from above and the right. */
const FACING_WINDING: number[] = (() => {
  const seen: Shape = {
    id: '',
    kind: 'box',
    box: [0, 0, 10, 10],
    angle: 0,
    face: null,
    solid: { thickness: 10, tip: 20, swing: 25 },
    points: null,
    sides: null,
    depth: 0,
    role: null,
    model: null,
  };
  const corners = solidCorners(seen, seen.solid!);
  return solidFaces(seen, seen.solid!).map((face, index) => {
    const sign = Math.sign(signedArea(faceCornerIndexes(index).map((corner) => corners[corner])));
    return face.facing ? sign : -sign;
  });
})();

/** A polygon's signed area, positive for clockwise on screen (y down). */
function signedArea(points: CropPoint[]): number {
  return (
    points.reduce((sum, [x, y], i) => {
      const [nx, ny] = points[(i + 1) % points.length];
      return sum + x * ny - nx * y;
    }, 0) / 2
  );
}

/** A box's or a polygon's placed vertices, when it has enough to span an area; null otherwise. */
export function freePoints(shape: Shape): CropPoint[] | null {
  const points = shape.points;
  const placeable = shape.kind === 'box' || shape.kind === 'polygon';
  return placeable && points && points.length >= 3 ? points : null;
}

/** The box round points: [center x, center y, width, height]. */
export function boxOf(points: CropPoint[]): CropBox {
  const xs = points.map(([x]) => x);
  const ys = points.map(([, y]) => y);
  const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
  return [(x0 + x1) / 2, (y0 + y1) / 2, x1 - x0, y1 - y0];
}

/** A shape given placed vertices: its box becomes the box round them, its turn and third face none. */
export function withPoints(shape: Shape, points: CropPoint[]): Shape {
  return { ...shape, points, box: boxOf(points), angle: 0, face: null };
}

/** A shape with one vertex moved to a point; the others stay. */
export function movedVertex(
  shape: Shape,
  points: CropPoint[],
  index: number,
  point: CropPoint,
): Shape {
  return withPoints(
    shape,
    points.map((vertex, i) => (i === index ? point : vertex)),
  );
}

/** Each side's (a 3D box's face's) vertex indexes: index as `flatSides`, or as a solid's faces. */
function sidesOf(points: CropPoint[]): number[][] {
  return points.length === SOLID_POINTS ? [0, 1, 2, 3, 4, 5].map(faceCornerIndexes) : FLAT_SIDES;
}

/** Whether a 3D box's face faces the camera, by the way its vertices wind on screen; a flat box's sides always do. */
function facing(points: CropPoint[], side: number): boolean {
  if (points.length !== SOLID_POINTS) return true;
  const area = signedArea(faceCornerIndexes(side).map((corner) => points[corner]));
  return Math.sign(area) === FACING_WINDING[side];
}

/** A placed box's side handles: each side's (each face's) middle. */
export function freeSides(points: CropPoint[]): SideHandle[] {
  return sidesOf(points).map((indexes, side) => {
    const [x, y] = indexes.reduce(
      ([sumX, sumY], index) => [sumX + points[index][0], sumY + points[index][1]],
      [0, 0],
    );
    return { point: [x / indexes.length, y / indexes.length], side, seen: facing(points, side) };
  });
}

/**
 * A placed box with one side (a 3D box's face) moved by a drag ([dx, dy] from where it was at the press): its vertices
 * move together; with `mirror` the opposite side moves the other way.
 */
export function movedSide(
  shape: Shape,
  points: CropPoint[],
  side: number,
  [dx, dy]: CropPoint,
  mirror: boolean,
): Shape {
  const sides = sidesOf(points);
  const moving = new Set(sides[side]);
  const opposite = new Set(mirror ? sides[side ^ 1] : []);
  return withPoints(
    shape,
    points.map(([x, y], i): CropPoint => {
      if (moving.has(i)) return [x + dx, y + dy];
      if (opposite.has(i)) return [x - dx, y - dy];
      return [x, y];
    }),
  );
}

/** Whether a point is on a placed box, `slack` pixels round it counting too. */
export function onFree(points: CropPoint[], point: CropPoint, slack: number): boolean {
  const hull = convexHull(points);
  return (
    inConvex(hull, point) ||
    hull.some((a, i) => segmentDistance(point, a, hull[(i + 1) % hull.length]) <= slack)
  );
}

/** A placed 3D box's faces toward the camera, lit by where they face on screen (up and to the left the lightest). */
export function freeFaces(points: CropPoint[]): SolidFace[] {
  if (points.length !== SOLID_POINTS) return [];
  const [cx, cy] = boxOf(points);
  return sidesOf(points).map((indexes, side) => {
    const corners = indexes.map((index) => points[index]);
    const [mx, my] = corners.reduce(([sx, sy], [x, y]) => [sx + x / 4, sy + y / 4], [0, 0]);
    const length = Math.hypot(mx - cx, my - cy) || 1;
    const light = 0.5 + (0.5 * (-(mx - cx) - (my - cy))) / (length * Math.SQRT2);
    return { corners, facing: facing(points, side), light };
  });
}

/** A placed box's edges: a flat box's four sides, a 3D box's twelve, each seen when a face it bounds faces the camera. */
export function freeEdges(points: CropPoint[]): SolidEdge[] {
  if (points.length !== SOLID_POINTS) {
    return points.map((from, i) => ({ from, to: points[(i + 1) % points.length], seen: true }));
  }
  return EDGES.map(([from, to]) => {
    const seen = [0, 1, 2, 3, 4, 5].some((side) => {
      const corners = faceCornerIndexes(side);
      return corners.includes(from) && corners.includes(to) && facing(points, side);
    });
    return { from: points[from], to: points[to], seen };
  });
}

/** Placed vertices moved by (dx, dy), crop pixels. */
export function movedPoints(points: CropPoint[], [dx, dy]: CropPoint): CropPoint[] {
  return points.map(([x, y]) => [x + dx, y + dy]);
}

/** Placed vertices scaled by a factor about a point. */
export function scaledPoints(points: CropPoint[], grow: number, [ax, ay]: CropPoint): CropPoint[] {
  return points.map(([x, y]) => [ax + (x - ax) * grow, ay + (y - ay) * grow]);
}

/** Placed vertices turned about the middle of the box round them (degrees, clockwise on screen). */
export function turnedPoints(points: CropPoint[], degrees: number): CropPoint[] {
  const [cx, cy] = boxOf(points);
  const turn = (degrees * Math.PI) / 180;
  const [sin, cos] = [Math.sin(turn), Math.cos(turn)];
  return points.map(([x, y]) => [
    cx + (x - cx) * cos - (y - cy) * sin,
    cy + (x - cx) * sin + (y - cy) * cos,
  ]);
}
