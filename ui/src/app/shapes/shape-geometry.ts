import { CropBox, FaceOffset, Shape } from '../api';

/**
 * Editing a crop's shapes with the pointer: where a shape's corners and handles are, what a drag does to it, and its
 * outline on a canvas. What a shape covers and hides is the core's (src/shapes.rs, through CoreModule.shapesVisible):
 * these are only the handles a finger takes. Coordinates are crop pixels; an angle is in degrees, clockwise on
 * screen.
 */

/** A point on a crop, in its pixels. */
export type CropPoint = [x: number, y: number];

/** A point in a shape's own frame: along its width, across it. */
type OwnPoint = [along: number, across: number];

/** A shape's smallest side when resized, in crop pixels. */
export const MIN_SIDE_PX = 2;

const radians = (degrees: number) => (degrees * Math.PI) / 180;

/** A point of a shape's own frame, on the crop. */
function toCrop(shape: Shape, [along, across]: OwnPoint): CropPoint {
  const [cx, cy] = shape.box;
  const turn = radians(shape.angle);
  return [
    cx + along * Math.cos(turn) - across * Math.sin(turn),
    cy + along * Math.sin(turn) + across * Math.cos(turn),
  ];
}

/** A crop point in a shape's own frame. */
function toOwn(shape: Shape, [x, y]: CropPoint): OwnPoint {
  const [cx, cy] = shape.box;
  const turn = radians(shape.angle);
  const [dx, dy] = [x - cx, y - cy];
  return [dx * Math.cos(turn) + dy * Math.sin(turn), -dx * Math.sin(turn) + dy * Math.cos(turn)];
}

/** A shape's four corners on the crop: its frame's top left, top right, bottom right and bottom left. */
export function corners(shape: Shape): CropPoint[] {
  const [, , width, height] = shape.box;
  const [halfW, halfH] = [width / 2, height / 2];
  return [
    toCrop(shape, [-halfW, -halfH]),
    toCrop(shape, [halfW, -halfH]),
    toCrop(shape, [halfW, halfH]),
    toCrop(shape, [-halfW, halfH]),
  ];
}

/** A shape's far corners: its frame's corners moved by its third face; none for a flat shape. */
export function farCorners(shape: Shape): CropPoint[] {
  const face = shape.face;
  return face ? corners(shape).map(([x, y]) => [x + face[0], y + face[1]]) : [];
}

/** Whether a point is on a shape (its frame, or its far end's), `slack` pixels round it counting too. */
export function onShape(shape: Shape, point: CropPoint, slack: number): boolean {
  const [, , width, height] = shape.box;
  const inFrame = (at: CropPoint) => {
    const [along, across] = toOwn(shape, at);
    return Math.abs(along) <= width / 2 + slack && Math.abs(across) <= height / 2 + slack;
  };
  const face = shape.face;
  return inFrame(point) || (face !== null && inFrame([point[0] - face[0], point[1] - face[1]]));
}

/** Where a shape's turn handle is: above the middle of its frame's top edge. */
export function turnHandle(shape: Shape, reach: number): CropPoint {
  return toCrop(shape, [0, -shape.box[3] / 2 - reach]);
}

/** Where a shape's face handle is: its far end's middle, or beside its top right corner before it has one. */
export function faceHandle(shape: Shape, reach: number): CropPoint {
  const [cx, cy, width, height] = shape.box;
  if (shape.face) return [cx + shape.face[0], cy + shape.face[1]];
  return toCrop(shape, [width / 2 + reach, -height / 2 - reach]);
}

/**
 * A shape resized by one corner (0 to 3, as `corners`) dragged to a point: the opposite corner stays put. `even`: both
 * sides the same, the longer one (Shift: a perfect circle or square).
 */
export function resized(shape: Shape, corner: number, point: CropPoint, even = false): Shape {
  const signs: OwnPoint[] = [
    [-1, -1],
    [1, -1],
    [1, 1],
    [-1, 1],
  ];
  const [signW, signH] = signs[corner];
  const [, , width, height] = shape.box;
  const fixed: OwnPoint = [(-signW * width) / 2, (-signH * height) / 2];
  const [along, across] = toOwn(shape, point);
  const [pulledWidth, pulledHeight] = [
    Math.max(MIN_SIDE_PX, signW * (along - fixed[0])),
    Math.max(MIN_SIDE_PX, signH * (across - fixed[1])),
  ];
  const longer = Math.max(pulledWidth, pulledHeight);
  const [newWidth, newHeight] = even ? [longer, longer] : [pulledWidth, pulledHeight];
  const middle: OwnPoint = [fixed[0] + (signW * newWidth) / 2, fixed[1] + (signH * newHeight) / 2];
  const [cx, cy] = toCrop(shape, middle);
  return { ...shape, box: [cx, cy, newWidth, newHeight] };
}

/** A shape with both sides the same, their mean, about its middle: a pill becomes a circle, a box a square. */
export function evened(shape: Shape): Shape {
  const [cx, cy, width, height] = shape.box;
  const side = (width + height) / 2;
  return { ...shape, box: [cx, cy, side, side] };
}

/** A shape moved by (dx, dy). */
export function moved(shape: Shape, [dx, dy]: CropPoint): Shape {
  const [cx, cy, width, height] = shape.box;
  return { ...shape, box: [cx + dx, cy + dy, width, height] };
}

/** A shape turned so its turn handle points at a point (its frame's top toward it), to a whole degree. */
export function turned(shape: Shape, [x, y]: CropPoint): Shape {
  const [cx, cy] = shape.box;
  const degrees = Math.round((Math.atan2(y - cy, x - cx) * 180) / Math.PI + 90);
  return turnedBy(shape, degrees - shape.angle);
}

/**
 * A shape turned about its middle by some degrees, clockwise on screen: its far end (a 3D shape's) turns with it, so
 * the whole shape turns.
 */
export function turnedBy(shape: Shape, degrees: number): Shape {
  const angle = (((shape.angle + degrees) % 360) + 360) % 360;
  if (!shape.face) return { ...shape, angle };
  const [dx, dy] = shape.face;
  const turn = radians(degrees);
  const face: FaceOffset = [
    dx * Math.cos(turn) - dy * Math.sin(turn),
    dx * Math.sin(turn) + dy * Math.cos(turn),
  ];
  return { ...shape, angle, face };
}

/** A shape given a third face whose middle is at a point. */
export function faced(shape: Shape, [x, y]: CropPoint): Shape {
  const [cx, cy] = shape.box;
  return { ...shape, face: [x - cx, y - cy] };
}

/**
 * A shape scaled about a point by a factor: its sides, its third face and its middle's distance from the point. No side
 * gets smaller than MIN_SIDE_PX.
 */
export function scaled(shape: Shape, factor: number, [x, y]: CropPoint): Shape {
  const [cx, cy, width, height] = shape.box;
  const grow = Math.max(factor, MIN_SIDE_PX / Math.min(width, height));
  const face: FaceOffset | null = shape.face && [shape.face[0] * grow, shape.face[1] * grow];
  return {
    ...shape,
    box: [x + (cx - x) * grow, y + (cy - y) * grow, width * grow, height * grow],
    face,
  };
}

/** The box round shapes' corners (and far faces): [center x, center y, width, height]. */
export function boxAround(shapes: readonly Shape[]): CropBox {
  const points = shapes.flatMap((shape) => [...corners(shape), ...farCorners(shape)]);
  const xs = points.map(([x]) => x);
  const ys = points.map(([, y]) => y);
  const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
  return [(x0 + x1) / 2, (y0 + y1) / 2, x1 - x0, y1 - y0];
}

/** A pill's turned round-ended frame as a path, moved by an offset (its far end's: its third face). */
function tracePill(context: CanvasRenderingContext2D, shape: Shape, [dx, dy]: FaceOffset): void {
  const [cx, cy, width, height] = shape.box;
  context.save();
  context.translate(cx + dx, cy + dy);
  context.rotate(radians(shape.angle));
  context.roundRect(-width / 2, -height / 2, width, height, Math.min(width, height) / 2);
  context.restore();
}

/**
 * The two lines joining a pill to its far end: its edges along the face, where the line through each side of the pill
 * parallel to the face touches it.
 */
function pillSides(shape: Shape, [dx, dy]: FaceOffset): CropPoint[] {
  const [, , width, height] = shape.box;
  const radius = Math.min(width, height) / 2;
  const half = Math.max(width, height) / 2 - radius;
  const ends = [-half, half].map((along) =>
    toCrop(shape, width >= height ? [along, 0] : [0, along]),
  );
  const length = Math.hypot(dx, dy) || 1;
  const [nx, ny] = [-dy / length, dx / length];
  const reach = ([x, y]: CropPoint) => x * nx + y * ny;
  const [low, high] = reach(ends[0]) <= reach(ends[1]) ? ends : [ends[1], ends[0]];
  return [
    [high[0] + radius * nx, high[1] + radius * ny],
    [low[0] - radius * nx, low[1] - radius * ny],
  ];
}

/**
 * A shape's outline as a path, the context drawing in crop pixels: a pill as its turned round-ended frame, a box as
 * its turned frame, and with a third face its far end and the edges joining them (a cube's or a cylinder's
 * wireframe).
 */
export function tracePath(context: CanvasRenderingContext2D, shape: Shape): void {
  context.beginPath();
  if (shape.kind === 'pill') {
    tracePill(context, shape, [0, 0]);
    const face = shape.face;
    if (!face) return;
    tracePill(context, shape, face);
    for (const [x, y] of pillSides(shape, face)) {
      context.moveTo(x, y);
      context.lineTo(x + face[0], y + face[1]);
    }
    return;
  }
  const near = corners(shape);
  const far = farCorners(shape);
  for (const face of far.length ? [near, far] : [near]) {
    face.forEach(([x, y], i) => (i ? context.lineTo(x, y) : context.moveTo(x, y)));
    context.closePath();
  }
  near.forEach(([x, y], i) => {
    if (!far.length) return;
    context.moveTo(x, y);
    context.lineTo(far[i][0], far[i][1]);
  });
}
