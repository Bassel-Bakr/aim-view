import { Report, Tracks } from '../../api';
import { FaintHover } from '../../services/faint-cutoff';
import { Point, toPx } from '../track';

/** How the cut-off draws on the video: its colors, font and sizes, from its tokens (themes/faint.scss). */
export interface FaintStyle {
  font: string;
  ring: string;
  picked: string;
  radius: number;
  labelBg: string;
  hoverBg: string;
  text: string;
  textOut: string;
}

export function readFaintStyle(element: Element): FaintStyle {
  const css = getComputedStyle(element);
  const token = (name: string) => css.getPropertyValue(name).trim();
  return {
    font: token('--overlay-font'),
    ring: token('--overlay-faint-ring'),
    picked: token('--overlay-faint-picked'),
    radius: Number(token('--overlay-faint-ring-radius')),
    labelBg: token('--overlay-faint-label-bg'),
    hoverBg: token('--overlay-faint-hover-bg'),
    text: token('--overlay-faint-text'),
    textOut: token('--overlay-faint-text-out'),
  };
}

/** What the cut-off shows on a frame: the tracks it leaves out, the one picked, the scores, and the point hovered. */
export interface FaintLayer {
  scores: ReadonlyMap<number, number>;
  dropped: ReadonlySet<number>;
  highlight: number | null;
  showScores: boolean;
  hover: FaintHover | null;
}

const DASH = [2, 3];
const LINE = 1.2;
const LINE_PICKED = 2;
const LABEL_X = 10;
const LABEL_Y = 16;
const LABEL_H = 14;
const LABEL_PAD = 3;
/** A score's text sits this far above its label's bottom padding, in pixels. */
const LABEL_TEXT_LIFT = 2;
const HOVER_X = 12;
const HOVER_Y = 8;
const HOVER_H = 18;
const HOVER_PAD = 5;
/** How near the mouse a track is pointed at, in pixels. */
const POINT_RADIUS = 12;

/** A track's score as the strip writes it; – for a track too short to have one. */
function scoreText(score: number | undefined): string {
  return score === undefined ? '–' : score.toFixed(2);
}

/** A ring round a track the cut-off leaves out (dashed) or the one picked in the strip, and its score beside it. */
function drawTrackMark(
  context: CanvasRenderingContext2D,
  style: FaintStyle,
  layer: FaintLayer,
  id: number,
  [x, y]: Point,
): void {
  const out = layer.dropped.has(id);
  const picked = id === layer.highlight;
  if (out || picked) {
    context.strokeStyle = picked ? style.picked : style.ring;
    context.lineWidth = picked ? LINE_PICKED : LINE;
    context.setLineDash(picked ? [] : DASH);
    context.beginPath();
    context.arc(x, y, style.radius, 0, 2 * Math.PI);
    context.stroke();
    context.setLineDash([]);
  }
  if (layer.showScores || picked) {
    const text = scoreText(layer.scores.get(id));
    context.fillStyle = style.labelBg;
    const labelWidth = context.measureText(text).width + 2 * LABEL_PAD;
    context.fillRect(x + LABEL_X, y - LABEL_Y, labelWidth, LABEL_H);
    context.fillStyle = out ? style.textOut : style.text;
    context.fillText(text, x + LABEL_X + LABEL_PAD, y - LABEL_PAD - LABEL_TEXT_LIFT);
  }
}

/** The hovered point's scores beside it: to its right, or to its left where the video has no room for them. */
function drawHover(context: CanvasRenderingContext2D, style: FaintStyle, hover: FaintHover): void {
  const width = context.measureText(hover.text).width + 2 * HOVER_PAD;
  const fitsRight = hover.x + HOVER_X + width <= context.canvas.clientWidth;
  const x = fitsRight ? hover.x + HOVER_X : hover.x - HOVER_X - width;
  context.fillStyle = style.hoverBg;
  context.fillRect(x, hover.y + HOVER_Y, width, HOVER_H);
  context.fillStyle = style.text;
  context.fillText(hover.text, x + HOVER_PAD, hover.y + HOVER_Y + HOVER_H - HOVER_PAD);
}

/**
 * The tracks the cut-off leaves out, on the frame shown: dimmed dashed rings, so the user sees what it removes. With
 * the scores shown, every track's score beside it; the track picked in the strip highlighted; the point under the mouse
 * with that frame's own score.
 */
export function drawFaint(
  context: CanvasRenderingContext2D,
  report: Report,
  all: Tracks,
  frame: number,
  scale: number,
  style: FaintStyle,
  layer: FaintLayer,
): void {
  const trackFrame = all.frames[frame];
  if (!trackFrame) return;
  context.font = style.font;
  context.textBaseline = 'alphabetic';
  for (const [id, x, y] of trackFrame.t) {
    drawTrackMark(context, style, layer, id, toPx(report.geometry, x, y, scale));
  }
  const hover = layer.hover;
  if (hover && hover.frame === frame) drawHover(context, style, hover);
}

/** The track the mouse points at on the frame shown (within 12 pixels), with its frame's score and its own. */
export function pointedTrack(
  report: Report,
  all: Tracks,
  frame: number,
  scale: number,
  mouseX: number,
  mouseY: number,
  scores: ReadonlyMap<number, number>,
): FaintHover | null {
  const trackFrame = all.frames[frame];
  if (!trackFrame) return null;
  let best: FaintHover | null = null;
  let bestDistancePx = Infinity;
  for (let index = 0; index < trackFrame.t.length; index++) {
    const [id, xDeg, yDeg] = trackFrame.t[index];
    const [x, y] = toPx(report.geometry, xDeg, yDeg, scale);
    const distancePx = Math.hypot(x - mouseX, y - mouseY);
    if (distancePx > POINT_RADIUS || distancePx >= bestDistancePx) continue;
    bestDistancePx = distancePx;
    const frameScore = scoreText(trackFrame.s?.[index]);
    const text = `track ${id} · this frame ${frameScore} · track ${scoreText(scores.get(id))}`;
    best = { id, x, y, frame, text };
  }
  return best;
}
