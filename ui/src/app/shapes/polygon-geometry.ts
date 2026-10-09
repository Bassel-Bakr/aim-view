/**
 * A polygon's vertices on the Crops page: a regular one's, inscribed in the ellipse of its turned
 * frame as the core places them (src/shapes.rs `polygon_vertices`), and the switch between a
 * uniform polygon and one placed vertex by vertex (Shape.points), each way. Crop pixels; an angle
 * is in degrees, clockwise on screen. Out: shape-geometry.ts, the stage and the tools.
 */

import { CropBox, Shape } from '../api';
import { boxOf, withPoints } from './free-geometry';
import type { CropPoint } from './shape-geometry';

/** A polygon's fewest sides (the core's MIN_SIDES). */
export const MIN_SIDES = 3;
/** The most sides the tools give a polygon. */
export const MAX_TOOL_SIDES = 12;
/** The most sides the core draws a polygon with (its MAX_SIDES). */
const MAX_SIDES = 64;
/** A polygon's sides when it gives none (the core's DEFAULT_SIDES). */
export const DEFAULT_SIDES = 6;
/** A refitted polygon's smallest side, in crop pixels (shape-geometry's MIN_SIDE_PX). */
const MIN_FIT_SIDE_PX = 2;

/** A polygon's sides: its placed vertices' count, or its `sides` (DEFAULT_SIDES when none). */
export function sidesOf(shape: Shape): number {
  return shape.points?.length ?? shape.sides ?? DEFAULT_SIDES;
}

/**
 * A regular polygon's vertices on the crop, in order round it: evenly spaced on the ellipse of its turned frame, the
 * first at the frame's top, clockwise.
 */
export function polygonVertices(shape: Shape): CropPoint[] {
  const [cx, cy, width, height] = shape.box;
  const sides = Math.min(MAX_SIDES, Math.max(MIN_SIDES, shape.sides ?? DEFAULT_SIDES));
  const turn = (shape.angle * Math.PI) / 180;
  return Array.from({ length: sides }, (_unused, vertex): CropPoint => {
    const at = (2 * Math.PI * vertex) / sides - Math.PI / 2;
    const [along, across] = [(width / 2) * Math.cos(at), (height / 2) * Math.sin(at)];
    return [
      cx + along * Math.cos(turn) - across * Math.sin(turn),
      cy + along * Math.sin(turn) + across * Math.cos(turn),
    ];
  });
}

/**
 * A polygon placed vertex by vertex as its frame: the box round its vertices, unturned (where its frame handles sit and
 * what they resize).
 */
export function placedFrame(shape: Shape): Shape {
  return shape.points ? { ...shape, box: boxOf(shape.points), angle: 0, face: null } : shape;
}

/**
 * A polygon placed vertex by vertex fitted to a new frame (as `placedFrame`'s): each vertex keeps its place in the
 * frame, so the layout of its vertices is kept, only scaled along each side (a frame side of no size stays put).
 */
export function reframed(shape: Shape, [cx, cy, width, height]: CropBox): Shape {
  const points = shape.points;
  if (!points) return shape;
  const [oldX, oldY, oldWidth, oldHeight] = boxOf(points);
  const placed = (value: number, from: number, to: number, oldSide: number, side: number) =>
    oldSide > 0 ? to + ((value - from) * side) / oldSide : to;
  return withPoints(
    shape,
    points.map(([x, y]): CropPoint => [
      placed(x, oldX, cx, oldWidth, width),
      placed(y, oldY, cy, oldHeight, height),
    ]),
  );
}

/** A uniform polygon made one placed vertex by vertex, from where its vertices are. */
export function placedPolygon(shape: Shape): Shape {
  if (shape.kind !== 'polygon' || shape.points) return shape;
  return withPoints({ ...shape, sides: sidesOf(shape) }, polygonVertices(shape));
}

/**
 * A polygon placed vertex by vertex made uniform again: the regular polygon of as many sides nearest its vertices (least
 * squares: its middle their mean, its size and turn from their offsets taken round by each vertex's place).
 */
export function refitPolygon(shape: Shape): Shape {
  const points = shape.points;
  if (shape.kind !== 'polygon' || !points) return shape;
  const count = points.length;
  const [cx, cy] = points.reduce(([sx, sy], [x, y]) => [sx + x / count, sy + y / count], [0, 0]);
  const [re, im] = points.reduce(
    ([sumRe, sumIm], [x, y], vertex) => {
      const back = (-2 * Math.PI * vertex) / count;
      const [dx, dy] = [x - cx, y - cy];
      return [
        sumRe + (dx * Math.cos(back) - dy * Math.sin(back)) / count,
        sumIm + (dx * Math.sin(back) + dy * Math.cos(back)) / count,
      ];
    },
    [0, 0],
  );
  const side = Math.max(MIN_FIT_SIDE_PX, 2 * Math.hypot(re, im));
  const angle = ((((Math.atan2(im, re) * 180) / Math.PI + 90) % 360) + 360) % 360;
  return { ...shape, points: null, sides: count, box: [cx, cy, side, side], angle, face: null };
}
