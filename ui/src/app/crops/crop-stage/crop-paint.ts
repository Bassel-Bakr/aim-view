import { CropEntry, SceneView, Shape, Solid } from '../../api';
import { CropPoint, tracePath } from '../../shapes/shape-geometry';
import { capsuleOutline, capsuleRings, solidEdges, solidFaces } from '../../shapes/solid-geometry';

/** The light at which a face is neither lit nor shaded. */
const LIGHT_MIDDLE = 0.5;
import { CROP_SIDE } from '../crop-draft';
import { DraftScene } from '../crop-scene';
import { handled, handlesOf } from './crop-grip';

/**
 * Drawing the Crops page's stage on its canvas: the crop's picture, the pixels the core says the targets show, the
 * crossed-out model boxes, each shape's outline in the color of what it is, the joined targets' boxes, and the selected
 * shapes' handles. Colors and widths come from themes/crops.scss.
 */

/** Where the crop sits on the stage: CSS pixels per crop pixel, and its top left corner in CSS pixels. */
export interface StagePlace {
  scale: number;
  left: number;
  top: number;
}

/** The stage's colors and widths (CSS pixels), read from the page's tokens. */
export interface CropStyle {
  model: string;
  drawn: string;
  head: string;
  body: string;
  occluder: string;
  crossed: string;
  target: string;
  under: string;
  visible: string;
  handle: string;
  faceLight: string;
  faceShade: string;
  lineWidth: number;
  lineWidthSelected: number;
  handleRadius: number;
}

/** What the stage shows. The scene is null while the user peeks under the marks. */
export interface StagePicture {
  image: ImageBitmap | null;
  crop: CropEntry;
  scene: DraftScene | null;
  view: SceneView | null;
  mask: HTMLCanvasElement | null;
  selection: readonly string[];
  editing: boolean;
  /** The shape a drag on the wall is drawing. */
  sketch: Shape | null;
}

/** Reads the stage's tokens (themes/crops.scss) from an element under :root. */
export function readCropStyle(element: Element): CropStyle {
  const css = getComputedStyle(element);
  const token = (name: string) => css.getPropertyValue(`--crop-${name}`).trim();
  return {
    model: token('model'),
    drawn: token('drawn'),
    head: token('head'),
    body: token('body'),
    occluder: token('occluder'),
    crossed: token('crossed'),
    target: token('target'),
    under: token('under'),
    visible: token('visible'),
    handle: token('handle'),
    lineWidth: Number(token('line-width')),
    lineWidthSelected: Number(token('line-width-selected')),
    handleRadius: Number(token('handle-radius')),
    faceLight: token('face-light'),
    faceShade: token('face-shade'),
  };
}

/** The pixels a scene's targets show (run lengths, the first of pixels not set), tinted, at the crop's size. */
export function maskPicture(runs: readonly number[], color: string): HTMLCanvasElement {
  const canvas = document.createElement('canvas');
  [canvas.width, canvas.height] = [CROP_SIDE, CROP_SIDE];
  const context = canvas.getContext('2d');
  if (!context) return canvas;
  const pixels = context.createImageData(CROP_SIDE, CROP_SIDE);
  let at = 0;
  runs.forEach((length, i) => {
    if (i % 2) for (let pixel = at; pixel < at + length; pixel++) pixels.data[pixel * 4 + 3] = 255;
    at += length;
  });
  context.putImageData(pixels, 0, 0);
  context.globalCompositeOperation = 'source-in';
  context.fillStyle = color;
  context.fillRect(0, 0, CROP_SIDE, CROP_SIDE);
  return canvas;
}

/** A shape's color: what it is (an occluder, a head, a body), else where it came from (the model, or drawn). */
function colorOf(shape: Shape, occluders: readonly string[], style: CropStyle): string {
  if (occluders.includes(shape.id)) return style.occluder;
  if (shape.role === 'head') return style.head;
  if (shape.role === 'body') return style.body;
  return shape.model === null ? style.drawn : style.model;
}

/** Strokes the path traced, over a darker line so it shows on any wall; `width` in CSS pixels. */
function strokeTwice(
  context: CanvasRenderingContext2D,
  color: string,
  width: number,
  style: CropStyle,
  scale: number,
) {
  context.lineWidth = (width + 2) / scale;
  context.strokeStyle = style.under;
  context.stroke();
  context.lineWidth = width / scale;
  context.strokeStyle = color;
  context.stroke();
}

/** The model boxes crossed out: a faint dashed pill with a cross in it (a tap brings one back). */
function paintCrossed(
  context: CanvasRenderingContext2D,
  picture: StagePicture,
  scale: number,
  style: CropStyle,
) {
  for (const model of picture.scene?.crossed ?? []) {
    const box = picture.crop.boxes[model];
    if (!box) continue;
    const [cx, cy, width, height] = box;
    context.beginPath();
    context.roundRect(cx - width / 2, cy - height / 2, width, height, Math.min(width, height) / 2);
    context.setLineDash([3 / scale, 3 / scale]);
    strokeTwice(context, style.crossed, style.lineWidth, style, scale);
    context.setLineDash([]);
    const arm = Math.max(Math.min(width, height) / 4, 3 / scale);
    context.beginPath();
    context.moveTo(cx - arm, cy - arm);
    context.lineTo(cx + arm, cy + arm);
    context.moveTo(cx + arm, cy - arm);
    context.lineTo(cx - arm, cy + arm);
    strokeTwice(context, style.crossed, style.lineWidth, style, scale);
  }
}

/** The joined targets' boxes (round all their shapes), then every shape from the back; a hidden target's dashed. */
function paintShapes(
  context: CanvasRenderingContext2D,
  picture: StagePicture,
  scale: number,
  style: CropStyle,
) {
  const scene = picture.scene;
  if (!scene) return;
  const targets = picture.view?.targets ?? [];
  context.setLineDash([4 / scale, 4 / scale]);
  for (const target of targets.filter((one) => one.shapes.length > 1)) {
    const [cx, cy, width, height] = target.whole;
    context.beginPath();
    context.rect(cx - width / 2, cy - height / 2, width, height);
    strokeTwice(context, style.target, style.lineWidth, style, scale);
  }
  const hidden = new Set(targets.filter((one) => one.hidden).flatMap((one) => one.shapes));
  for (const shape of [...scene.shapes].sort((a, b) => a.depth - b.depth)) {
    const width = picture.selection.includes(shape.id) ? style.lineWidthSelected : style.lineWidth;
    const line: SolidLine = { color: colorOf(shape, scene.occluders, style), width, scale };
    if (shape.solid) {
      paintSolid(context, shape, shape.solid, line, style);
      continue;
    }
    tracePath(context, shape);
    context.setLineDash(hidden.has(shape.id) ? [6 / scale, 4 / scale] : []);
    strokeTwice(context, line.color, width, style, scale);
  }
  context.setLineDash([]);
  if (picture.sketch) {
    tracePath(context, picture.sketch);
    strokeTwice(context, style.drawn, style.lineWidth, style, scale);
  }
}

/** How a solid's lines are drawn: its color, its width and the stage's scale (screen pixels a crop pixel). */
interface SolidLine {
  color: string;
  width: number;
  scale: number;
}

/** Strokes the path solid, or dashed for what the camera does not see. */
function strokeSeen(
  context: CanvasRenderingContext2D,
  seen: boolean,
  line: SolidLine,
  style: CropStyle,
) {
  context.setLineDash(seen ? [] : [4 / line.scale, 3 / line.scale]);
  strokeTwice(context, line.color, line.width, style, line.scale);
  context.setLineDash([]);
}

/**
 * A solid shape: a box's faces toward the camera lit or shaded by where they face, its seen edges solid and its hidden
 * ones dashed; a capsule shaded across its axis like a cylinder, its near end ring solid and its far one dashed.
 */
function paintSolid(
  context: CanvasRenderingContext2D,
  shape: Shape,
  solid: Solid,
  line: SolidLine,
  style: CropStyle,
) {
  if (shape.kind === 'pill') {
    paintCapsule(context, shape, solid, line, style);
    return;
  }
  for (const face of solidFaces(shape, solid).filter((one) => one.facing)) {
    context.beginPath();
    face.corners.forEach(([x, y], i) => (i ? context.lineTo(x, y) : context.moveTo(x, y)));
    context.closePath();
    context.globalAlpha = Math.abs(face.light - LIGHT_MIDDLE) * 2;
    context.fillStyle = face.light > LIGHT_MIDDLE ? style.faceLight : style.faceShade;
    context.fill();
  }
  context.globalAlpha = 1;
  const edges = solidEdges(shape, solid);
  for (const seen of [false, true]) {
    context.beginPath();
    for (const edge of edges.filter((one) => one.seen === seen)) {
      context.moveTo(...edge.from);
      context.lineTo(...edge.to);
    }
    strokeSeen(context, seen, line, style);
  }
}

function paintCapsule(
  context: CanvasRenderingContext2D,
  shape: Shape,
  solid: Solid,
  line: SolidLine,
  style: CropStyle,
) {
  const outline = capsuleOutline(shape, solid);
  const rings = capsuleRings(shape, solid);
  const [{ center, across, tilt }] = rings;
  const [nx, ny] = [-Math.sin(tilt) * across, Math.cos(tilt) * across];
  const shading = context.createLinearGradient(
    center[0] - nx,
    center[1] - ny,
    center[0] + nx,
    center[1] + ny,
  );
  shading.addColorStop(0, style.faceShade);
  shading.addColorStop(0.35, style.faceLight);
  shading.addColorStop(1, style.faceShade);
  context.beginPath();
  outline.forEach(([x, y], i) => (i ? context.lineTo(x, y) : context.moveTo(x, y)));
  context.closePath();
  context.fillStyle = shading;
  context.fill();
  strokeSeen(context, true, line, style);
  for (const ring of rings) {
    context.beginPath();
    context.ellipse(...ring.center, ring.along, ring.across, ring.tilt, 0, 2 * Math.PI);
    strokeSeen(context, ring.near, line, style);
  }
}

/** A round handle at a screen point. */
function dot(
  context: CanvasRenderingContext2D,
  [x, y]: CropPoint,
  radius: number,
  style: CropStyle,
) {
  context.beginPath();
  context.arc(x, y, radius, 0, 2 * Math.PI);
  context.fillStyle = style.handle;
  context.fill();
  context.lineWidth = 1;
  context.strokeStyle = style.under;
  context.stroke();
}

/**
 * The selected shape's handles, at the same size on screen at any zoom: corners, the turn handle, the face's, and a
 * solid's tumble (a ringed dot) and thickness (a square).
 */
function paintHandles(
  context: CanvasRenderingContext2D,
  picture: StagePicture,
  place: StagePlace,
  style: CropStyle,
) {
  const onScreen = ([x, y]: CropPoint): CropPoint => [
    place.left + x * place.scale,
    place.top + y * place.scale,
  ];
  const radius = style.handleRadius;
  const shape = picture.scene && handled(picture.scene, picture.selection);
  if (shape) {
    const handles = handlesOf(shape, place.scale);
    const [first, second] = handles.corners.length
      ? handles.corners.map(onScreen)
      : [onScreen([shape.box[0], shape.box[1]]), onScreen([shape.box[0], shape.box[1]])];
    const turn = onScreen(handles.turn);
    context.beginPath();
    context.moveTo((first[0] + second[0]) / 2, (first[1] + second[1]) / 2);
    context.lineTo(turn[0], turn[1]);
    context.lineWidth = 1;
    context.strokeStyle = style.handle;
    context.stroke();
    for (const corner of handles.corners) dot(context, onScreen(corner), radius, style);
    dot(context, turn, radius, style);
    if (handles.face) {
      const [x, y] = onScreen(handles.face);
      context.fillStyle = style.handle;
      context.fillRect(x - radius, y - radius, 2 * radius, 2 * radius);
    }
    // a solid's sides: squares, hollow for the sides the camera does not see
    for (const side of handles.sides) {
      const [x, y] = onScreen(side.point);
      context.fillStyle = style.handle;
      context.strokeStyle = style.handle;
      context.lineWidth = 2;
      if (side.seen) context.fillRect(x - radius, y - radius, 2 * radius, 2 * radius);
      else context.strokeRect(x - radius, y - radius, 2 * radius, 2 * radius);
    }
    if (handles.tumble) {
      // a ball to roll: a ring round a dot
      const [x, y] = onScreen(handles.tumble);
      dot(context, [x, y], radius, style);
      context.beginPath();
      context.arc(x, y, 2 * radius, 0, 2 * Math.PI);
      context.lineWidth = 2;
      context.strokeStyle = style.handle;
      context.stroke();
    }
  }
}

/** Draws the stage: the context is in CSS pixels, cleared. */
export function paintStage(
  context: CanvasRenderingContext2D,
  picture: StagePicture,
  place: StagePlace,
  style: CropStyle,
): void {
  const side = CROP_SIDE * place.scale;
  context.imageSmoothingEnabled = false;
  if (picture.image) context.drawImage(picture.image, place.left, place.top, side, side);
  if (!picture.scene) return;
  if (picture.mask) context.drawImage(picture.mask, place.left, place.top, side, side);
  context.save();
  context.translate(place.left, place.top);
  context.scale(place.scale, place.scale);
  paintCrossed(context, picture, place.scale, style);
  paintShapes(context, picture, place.scale, style);
  context.restore();
  if (picture.editing) paintHandles(context, picture, place, style);
}
