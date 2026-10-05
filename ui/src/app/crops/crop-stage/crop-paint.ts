import { CropEntry, SceneView, Shape } from '../../api';
import { CropPoint, tracePath } from '../../shapes/shape-geometry';
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
    tracePath(context, shape);
    context.setLineDash(hidden.has(shape.id) ? [6 / scale, 4 / scale] : []);
    const width = picture.selection.includes(shape.id) ? style.lineWidthSelected : style.lineWidth;
    strokeTwice(context, colorOf(shape, scene.occluders, style), width, style, scale);
  }
  context.setLineDash([]);
  if (picture.sketch) {
    tracePath(context, picture.sketch);
    strokeTwice(context, style.drawn, style.lineWidth, style, scale);
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

/** The selected shape's handles, at the same size on screen at any zoom: corners, the turn handle, a box's face. */
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
    const [first, second] = handles.corners.map(onScreen);
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
