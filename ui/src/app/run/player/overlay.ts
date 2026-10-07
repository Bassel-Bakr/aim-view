/**
 * What the player draws over the video, on its canvas, for one frame.
 *
 * In: the review's report and tracks, the fastest-path analysis (fastest-path/path-analysis.ts),
 * the frame, the canvas's scale and the overlay's tokens (themes/player.scss).
 * Out: a clicking run's flick ring and paths, or a tracking run's target outlines and label, drawn
 * on the canvas's 2D context (player.ts calls these).
 */

import {
  ClickReport,
  Geometry,
  TrackFrame,
  TrackPoint,
  TrackReport,
  Tracks,
  TrackSummary,
} from '../../api';
import { formatMs } from '../../format';
import { FastestOrder } from '../fastest-path/order-solver';
import { NEW_MS, newCut, PathAnalysis } from '../fastest-path/path-analysis';
import { boxes, flickAt, nearest, Point, TargetBox, TargetShapeKind, toPx } from '../track';

/**
 * How the overlay draws: its colors, font and line widths, from the player's tokens
 * (themes/player.scss).
 */
export interface OverlayStyle {
  /** The labels' and the legend's font (`--overlay-font`). */
  font: string;
  /** The crosshair's ring on a clicking run. */
  crosshair: string;
  /** A tracking run's targets other than the one followed. */
  otherTarget: string;
  /** The ring round a clicking run's target. */
  ring: string;
  /** The followed target while the crosshair is on it. */
  onTarget: string;
  /** The followed target, and the dashed line to it, while the crosshair is off it. */
  offTarget: string;
  /** Behind a label's text. */
  labelBg: string;
  /** A label's text, and the numbers in the path dots. */
  labelText: string;
  /** The usual line width, in CSS pixels. */
  line: number;
  /**
   * The width of the lines that matter most (the followed target, the flick's ring), in CSS pixels.
   */
  lineStrong: number;
  /** The fastest path's color. */
  fastest: string;
  /** Your path's color. */
  mine: string;
  /** The font of the numbers in the path dots. */
  orderFont: string;
  /** The paths' dotted line width, in CSS pixels. */
  pathWidth: number;
  /** Behind the paths' legend. */
  legendBg: string;
}

/** Reads the overlay's tokens from the CSS variables in force on `el` (the canvas). */
export function readOverlayStyle(el: Element): OverlayStyle {
  const css = getComputedStyle(el);
  const token = (name: string) => css.getPropertyValue(name).trim();
  return {
    font: token('--overlay-font'),
    crosshair: token('--overlay-crosshair'),
    otherTarget: token('--overlay-other-target'),
    ring: token('--overlay-ring'),
    onTarget: token('--on-target'),
    offTarget: token('--off-target'),
    labelBg: token('--overlay-label-bg'),
    labelText: token('--overlay-label-text'),
    line: Number(token('--overlay-line-width')),
    lineStrong: Number(token('--overlay-line-width-strong')),
    fastest: token('--overlay-fastest'),
    mine: token('--overlay-mine'),
    orderFont: token('--overlay-order-font'),
    pathWidth: Number(token('--overlay-path-width')),
    legendBg: token('--overlay-legend-bg'),
  };
}

/** Frames before a flick starts and after its kill that the overlay still shows it. */
const NEAR_FLICK = 30;
/** The crosshair's ring on a clicking run, its radius in CSS pixels. */
const CROSSHAIR_RADIUS = 10;
/** The dashed line from the crosshair to a target: dash and gap, in CSS pixels. */
const DASH = [4, 4];
/** The ring round a clicking run's target, as a multiple of the target's radius. */
const RING_SCALE = 1.8;
/** The ring's smallest radius, in CSS pixels, so a far target's ring still shows. */
const RING_MIN = 6;
/** A tracked box's gap from the target's edge, in CSS pixels. */
const BOX_PAD = 1.5;
/** A label's padding, in CSS pixels. */
const LABEL_PAD = 4;
/** A label's height, in CSS pixels. */
const LABEL_HEIGHT = 16;

/** A label: `text` on its background box, whose top left corner is `LABEL_PAD` above the point. */
function label(
  context: CanvasRenderingContext2D,
  style: OverlayStyle,
  text: string,
  [x, y]: Point,
): void {
  context.font = style.font;
  context.fillStyle = style.labelBg;
  context.fillRect(x, y - LABEL_PAD, context.measureText(text).width + 2 * LABEL_PAD, LABEL_HEIGHT);
  context.fillStyle = style.labelText;
  context.textBaseline = 'middle';
  context.fillText(text, x + LABEL_PAD, y - LABEL_PAD + LABEL_HEIGHT / 2);
}

/** A dashed line between two points, in the context's stroke color and width. */
function dashedLine(context: CanvasRenderingContext2D, [x0, y0]: Point, [x1, y1]: Point): void {
  context.setLineDash(DASH);
  context.beginPath();
  context.moveTo(x0, y0);
  context.lineTo(x1, y1);
  context.stroke();
  context.setLineDash([]);
}

/**
 * A clicking run: the crosshair as a faint ring, and the flick's target as a ring with a dashed
 * line from the crosshair and its distance in degrees, from just before the flick until just after
 * its kill. `scale` is CSS pixels per pixel of the review's frame.
 */
export function drawClick(
  context: CanvasRenderingContext2D,
  report: ClickReport,
  frame: number,
  scale: number,
  style: OverlayStyle,
): void {
  const flick = flickAt(report.flicks, frame);
  if (!flick || frame < flick.start_frame - NEAR_FLICK || frame > flick.kill_frame + NEAR_FLICK)
    return;
  const geometry = report.geometry;
  const center = toPx(geometry, 0, 0, scale);
  context.strokeStyle = style.crosshair;
  context.lineWidth = style.line;
  context.beginPath();
  context.arc(center[0], center[1], CROSSHAIR_RADIUS, 0, 2 * Math.PI);
  context.stroke();
  const point = report.paths[String(flick.kill_number)]?.find(
    (pathPoint) => pathPoint[0] === frame,
  );
  if (!point) return;
  const target = toPx(geometry, point[1], point[2], scale);
  const edge = toPx(geometry, point[1] + report.summary.radius, point[2], scale)[0];
  const radius = Math.max(RING_MIN, (edge - target[0]) * RING_SCALE);
  context.strokeStyle = style.ring;
  context.lineWidth = style.lineStrong;
  context.beginPath();
  context.arc(target[0], target[1], radius, 0, 2 * Math.PI);
  context.stroke();
  dashedLine(context, center, target);
  const distance = `${Math.hypot(point[1], point[2]).toFixed(1)}°`;
  label(context, style, distance, [target[0] + radius + LABEL_PAD, target[1]]);
}

/**
 * A tracking run: every target in its bots' hitbox shape (an ellipse for spheres, a capsule, a box;
 * a box without a hitbox), the one the review follows (nearest the crosshair) green while the
 * crosshair is on it and orange with a dashed line and its distance when not; "switching" after a
 * bot's death. Nothing outside the run's marked start and end.
 */
export function drawTrack(
  context: CanvasRenderingContext2D,
  report: TrackReport,
  tracks: Tracks,
  frame: number,
  scale: number,
  style: OverlayStyle,
): void {
  const summary = report.summary;
  const trackFrame = tracks.frames[frame];
  if (!trackFrame || outsideRun(summary, frame)) return;
  const geometry = report.geometry;
  const targetBoxes = boxes(trackFrame, report.hitbox ?? null);
  const best = nearest(targetBoxes);
  const corner = drawBoxes(context, geometry, scale, style, targetBoxes, best);
  const center = toPx(geometry, 0, 0, scale);
  if (best && !best.inside) {
    context.strokeStyle = style.offTarget;
    context.lineWidth = style.line;
    dashedLine(context, center, toPx(geometry, best.x, best.y, scale));
  }
  const text = trackLabel(summary, frame, best);
  if (text) {
    const [x, y] = corner ?? center;
    label(context, style, text, [x + LABEL_PAD + BOX_PAD, y]);
  }
}

/** The frame is before the run's marked start or from its marked end on. */
function outsideRun(summary: TrackSummary, frame: number): boolean {
  if (summary.start === null || summary.end === null) return false;
  return frame < summary.start || frame >= summary.end;
}

/** The followed target's box is in its on or off color, the others in their own. */
function boxColor(style: OverlayStyle, box: TargetBox, followed: boolean): string {
  if (!followed) return style.otherTarget;
  return box.inside ? style.onTarget : style.offTarget;
}

/**
 * Every target in its shape. Returns the followed target's lower right corner, where its label
 * goes.
 */
function drawBoxes(
  context: CanvasRenderingContext2D,
  geometry: Geometry,
  scale: number,
  style: OverlayStyle,
  targetBoxes: TargetBox[],
  best: TargetBox | null,
): Point | null {
  let corner: Point | null = null;
  for (const box of targetBoxes) {
    const { halfWidthDeg, halfHeightDeg } = box.shape;
    const [x0, y0] = toPx(geometry, box.x - halfWidthDeg, box.y + halfHeightDeg, scale);
    const [x1, y1] = toPx(geometry, box.x + halfWidthDeg, box.y - halfHeightDeg, scale);
    const followed = box === best;
    context.strokeStyle = boxColor(style, box, followed);
    context.lineWidth = followed ? style.lineStrong : style.line;
    strokeShape(
      context,
      box.shape.kind,
      [x0 - BOX_PAD, y0 - BOX_PAD],
      [x1 + BOX_PAD, y1 + BOX_PAD],
    );
    if (followed) corner = [x1, y1];
  }
  return corner;
}

/** A target's outline between two corners: a box, the ellipse inside it, or an upright capsule. */
function strokeShape(
  context: CanvasRenderingContext2D,
  kind: TargetShapeKind,
  [left, top]: Point,
  [right, bottom]: Point,
): void {
  const width = right - left;
  const height = bottom - top;
  if (kind === 'box') {
    context.strokeRect(left, top, width, height);
    return;
  }
  context.beginPath();
  if (kind === 'ellipse')
    context.ellipse(left + width / 2, top + height / 2, width / 2, height / 2, 0, 0, 2 * Math.PI);
  else context.roundRect(left, top, width, height, width / 2);
  context.stroke();
}

/**
 * The followed box's label: switching after a death, on, or how far off its edge the crosshair is.
 */
function trackLabel(summary: TrackSummary, frame: number, best: TargetBox | null): string {
  if (summary.switches.some(([a, b]) => frame >= a && frame < b)) return 'switching';
  if (!best) return '';
  return best.inside ? 'on' : `${best.outsideDeg.toFixed(2)}° off its edge`;
}

/** Which of the two paths to draw. */
export interface PathFlags {
  /** The fastest order through the targets on screen. */
  fastest: boolean;
  /** The order you killed them in. */
  mine: boolean;
}

/** A line of the paths' legend: its color and text. */
interface LegendRow {
  /** The swatch's color: the path's. */
  color: string;
  /** The path's predicted time, in words. */
  text: string;
}

/** The numbered dot on each target of a path, its radius in CSS pixels. */
const DOT_RADIUS = 5;
/** The path's line: a dash this short with round caps draws dots, about 3 CSS pixels apart. */
const DOT_DASH = [0.1, 3];
/** The path line's opacity, so the targets show through. */
const PATH_ALPHA = 0.7;
/** The numbered dots' opacity. */
const DOT_ALPHA = 0.8;
/** Your path's numbers sit below left of a target, the fastest's above right: both show on one. */
const MINE_OFFSET: Point = [-8, 8];
/** The fastest path's numbers sit above right of a target. */
const FASTEST_OFFSET: Point = [8, -8];
/** The legend's margin and padding, in CSS pixels. */
const LEGEND_X = 6;
/** The height of a legend line, in CSS pixels. */
const LEGEND_ROW = 15;
/** The side of a legend line's color swatch, in CSS pixels. */
const LEGEND_SWATCH = 7;

/**
 * A path from the crosshair through the targets in `order`, as a dotted line in `color`, with a
 * numbered dot on each target, set off from it by the offset (CSS pixels).
 */
function path(
  context: CanvasRenderingContext2D,
  report: ClickReport,
  scale: number,
  style: OverlayStyle,
  order: TrackPoint[],
  color: string,
  [offsetX, offsetY]: Point,
): void {
  const geometry = report.geometry;
  const pointsPx = order.map((point) => toPx(geometry, point[1], point[2], scale));
  context.strokeStyle = color;
  context.lineWidth = style.pathWidth;
  context.lineCap = 'round';
  context.setLineDash(DOT_DASH);
  context.globalAlpha = PATH_ALPHA;
  context.beginPath();
  const [centerX, centerY] = toPx(geometry, 0, 0, scale);
  context.moveTo(centerX, centerY);
  for (const [x, y] of pointsPx) context.lineTo(x, y);
  context.stroke();
  context.lineCap = 'butt';
  context.setLineDash([]);
  context.globalAlpha = DOT_ALPHA;
  context.font = style.orderFont;
  context.textAlign = 'center';
  context.textBaseline = 'middle';
  pointsPx.forEach(([x, y], i) => {
    context.fillStyle = color;
    context.beginPath();
    context.arc(x + offsetX, y + offsetY, DOT_RADIUS, 0, 2 * Math.PI);
    context.fill();
    context.fillStyle = style.labelText;
    context.fillText(String(i + 1), x + offsetX, y + offsetY);
  });
  context.globalAlpha = 1;
  context.textAlign = 'left';
  context.textBaseline = 'alphabetic';
}

/**
 * The fastest order through the targets on screen (green), and the order you killed them in
 * (orange), each with its predicted time in a legend. Only during the run: not on the countdown
 * before it, nor after the last kill. Targets new to the screen are left out, as in the path cost.
 */
export function drawPaths(
  context: CanvasRenderingContext2D,
  report: ClickReport,
  tracks: Tracks,
  frame: number,
  scale: number,
  style: OverlayStyle,
  analysis: PathAnalysis,
  show: PathFlags,
): void {
  const trackFrame = tracks.frames[frame];
  if (!trackFrame || frame > analysis.lastKill || frame < analysis.firstStart) return;
  const candidates = pathTargets(report, trackFrame, frame, analysis);
  const best = analysis.solver.fastestOrder(candidates);
  if (!best) return;
  const yours = yourOrder(candidates, analysis, frame);
  const showMine = show.mine && yours.length > 0;
  const targetCount = best.order.length;
  const targets = `the ${targetCount} target${targetCount > 1 ? 's' : ''} on screen`;
  const rows: LegendRow[] = [];
  if (showMine) path(context, report, scale, style, yours, style.mine, MINE_OFFSET);
  if (show.fastest) {
    path(context, report, scale, style, best.order, style.fastest, FASTEST_OFFSET);
    const skipped = trackFrame.t.length - candidates.length;
    const left = skipped ? ` (${skipped} new left out)` : '';
    rows.push({
      color: style.fastest,
      text: `Fastest path through ${targets}${left}: about ${formatMs(best.seconds)}`,
    });
  }
  if (showMine) rows.push(yourRow(style, analysis, yours, best, show.fastest ? 'them' : targets));
  if (rows.length) drawLegend(context, style, rows);
}

/**
 * The targets the paths go through: those on screen that are not new to it. Where only new ones are
 * on screen (one at a time), there is no choice to leave out, so all of them.
 */
function pathTargets(
  report: ClickReport,
  trackFrame: TrackFrame,
  frame: number,
  analysis: PathAnalysis,
): TrackPoint[] {
  const flick = flickAt(report.flicks, frame);
  const cut =
    flick && flick.kill_frame >= frame
      ? newCut(flick, report.fps)
      : frame - (NEW_MS / 1000) * report.fps;
  const choices = trackFrame.t.filter(
    (target) => (analysis.firstSeen.get(target[0]) ?? Infinity) <= cut,
  );
  return choices.length ? choices : trackFrame.t;
}

/** The targets you killed from this frame on, in the order you killed them. */
function yourOrder(candidates: TrackPoint[], analysis: PathAnalysis, frame: number): TrackPoint[] {
  const killFrame = (target: TrackPoint, missing: number) =>
    analysis.killOf.get(target[0])?.kill_frame ?? missing;
  return candidates
    .filter((target) => killFrame(target, -1) >= frame)
    .sort((a, b) => killFrame(a, 0) - killFrame(b, 0));
}

/** Your path's legend line: its predicted time, and how much slower than the fastest it is. */
function yourRow(
  style: OverlayStyle,
  analysis: PathAnalysis,
  yours: TrackPoint[],
  best: FastestOrder,
  what: string,
): LegendRow {
  const allOfThem = yours.length === best.order.length;
  const mine = analysis.solver.seconds(yours.length, analysis.solver.pathUnits(yours));
  const fastest = allOfThem ? best.seconds : (analysis.solver.fastestOrder(yours)?.seconds ?? mine);
  const slowerMs = Math.round(1000 * (mine - fastest));
  const through = allOfThem
    ? `Your path through ${what}`
    : `Your path through the ${yours.length} of ${what} you killed`;
  const against = slowerMs <= 0 ? 'the fastest' : `${slowerMs} ms slower than the fastest`;
  return { color: style.mine, text: `${through}: about ${formatMs(mine)}, ${against}` };
}

/** The legend in the top left corner: a swatch and a line of text for each path. */
function drawLegend(
  context: CanvasRenderingContext2D,
  style: OverlayStyle,
  rows: LegendRow[],
): void {
  context.font = style.font;
  const width = Math.max(...rows.map((row) => context.measureText(row.text).width)) + 4 * LEGEND_X;
  context.fillStyle = style.legendBg;
  context.fillRect(LEGEND_X, LEGEND_X, width, LEGEND_X + LEGEND_ROW * rows.length);
  rows.forEach((row, i) => {
    const y = 2 * LEGEND_X + LEGEND_ROW * i;
    context.fillStyle = row.color;
    context.fillRect(2 * LEGEND_X, y + 1, LEGEND_SWATCH, LEGEND_SWATCH);
    context.fillStyle = style.labelText;
    context.textBaseline = 'top';
    context.fillText(row.text, 2 * LEGEND_X + LEGEND_SWATCH + LEGEND_X, y);
  });
  context.textBaseline = 'alphabetic';
}
