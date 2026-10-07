/**
 * A tracking run's timeline, drawn on its canvas.
 *
 * In: the run moment by moment (track.ts `timeline`: each frame's state and distance off the bot,
 * the bots' deaths) and the timeline's tokens (themes/timeline.scss).
 * Out: the distance chart, the on and off strip, the deaths and the seconds, drawn on the canvas's
 * 2D context (timeline.ts calls it).
 */

import { Timeline, TrackState } from '../track';

/**
 * How the timeline draws: colors, font and the strip's height, from its tokens
 * (themes/timeline.scss).
 */
export interface TimelineStyle {
  /** The grid lines, and the strip where no bot was found. */
  grid: string;
  /** The strip where the crosshair was on the bot. */
  on: string;
  /** The distance areas, and the strip where the crosshair was off the bot. */
  off: string;
  /** The strip while switching after a bot's death. */
  switching: string;
  /** The scale's labels. */
  text: string;
  /** Behind the scale's labels. */
  labelBg: string;
  /** A bot's death mark. */
  death: string;
  /** The labels' font. */
  font: string;
  /** The strip's height, in CSS pixels. */
  strip: number;
}

/** Reads the timeline's tokens from the CSS variables in force on `element` (the canvas). */
export function readTimelineStyle(element: Element): TimelineStyle {
  const css = getComputedStyle(element);
  const token = (name: string) => css.getPropertyValue(name).trim();
  return {
    grid: token('--grid'),
    on: token('--on-target'),
    off: token('--off-target'),
    switching: token('--text-muted'),
    text: token('--text-secondary'),
    labelBg: token('--overlay-label-bg'),
    death: token('--timeline-death'),
    font: token('--timeline-font'),
    strip: Number(token('--timeline-strip-height')),
  };
}

/** Space above the chart, in CSS pixels. */
const TOP = 6;
/** Space between the chart and the strip, in CSS pixels. */
const STRIP_GAP = 8;
/** Space under the strip for the seconds, in CSS pixels. */
const AXIS = 30;
/** The dashed grid lines' dash and gap, in CSS pixels. */
const DASH = [3, 4];
/** The grid's lines: at no distance, half the scale and its top. */
const GRID_SHARES = [0, 0.5, 1];
/** Lines on the canvas sit on a pixel's center, so they stay sharp. */
const HALF_PIXEL = 0.5;
/** The furthest distance's area is this opaque: light, behind the average's. */
const MAX_ALPHA = 0.35;
/** The average distance's area is this opaque. */
const MEAN_ALPHA = 0.9;
/** A scale label's padding, in CSS pixels. */
const LABEL_PAD = 3;
/** A scale label's height, in CSS pixels. */
const LABEL_HEIGHT = 14;
/** A death's mark is this wide, in CSS pixels. */
const DEATH_WIDTH = 1.5;
/** A death's mark rises this far above the strip, in CSS pixels. */
const DEATH_RISE = 4;
/** Seconds between the seconds' labels. */
const STEP_SECONDS = 10;
/** Seconds between the seconds' labels on a run longer than `LONG_RUN_SECONDS`. */
const LONG_STEP_SECONDS = 20;
/** A run longer than this, in seconds, gets the longer step between labels. */
const LONG_RUN_SECONDS = 90;
/** A seconds label this near the right edge, in CSS pixels, is right-aligned so it stays whole. */
const RIGHT_EDGE = 20;

/**
 * Calls back with the frames each pixel column covers: from firstFrame up to endFrame (at least one
 * frame), counted from the run's start.
 */
function columns(
  run: Timeline,
  widthPx: number,
  each: (firstFrame: number, endFrame: number, column: number) => void,
): void {
  for (let column = 0; column < widthPx; column++) {
    const firstFrame = Math.floor((column * run.frameCount) / widthPx);
    const endFrame = Math.max(
      firstFrame + 1,
      Math.floor(((column + 1) * run.frameCount) / widthPx),
    );
    each(firstFrame, endFrame, column);
  }
}

/** Per pixel column, the average and the furthest distance outside the bot's edge, in degrees. */
interface ColumnDistances {
  /** Each column's average distance over the frames with one; 0 when none has. */
  mean: number[];
  /** Each column's furthest distance; 0 when no frame has one. */
  furthest: number[];
}

/** The distances outside the bot's edge per pixel column, frames with none (NaN) left out. */
function columnDistances(run: Timeline, widthPx: number): ColumnDistances {
  const mean: number[] = [];
  const furthest: number[] = [];
  columns(run, widthPx, (firstFrame, endFrame) => {
    let sum = 0;
    let count = 0;
    let most = 0;
    for (let frame = firstFrame; frame < endFrame; frame++) {
      const distanceDeg = run.outsideDeg[frame];
      if (Number.isNaN(distanceDeg)) continue;
      sum += distanceDeg;
      count++;
      most = Math.max(most, distanceDeg);
    }
    mean.push(count ? sum / count : 0);
    furthest.push(most);
  });
  return { mean, furthest };
}

/** The chart's height in pixels at a distance in degrees. */
type ChartY = (distanceDeg: number) => number;

/**
 * The grid's lines across the chart: solid at no distance, dashed at half the scale and its top.
 */
function drawGrid(
  context: CanvasRenderingContext2D,
  run: Timeline,
  widthPx: number,
  style: TimelineStyle,
  yOf: ChartY,
): void {
  context.strokeStyle = style.grid;
  for (const share of GRID_SHARES) {
    context.setLineDash(share ? DASH : []);
    context.beginPath();
    context.moveTo(0, Math.round(yOf(share * run.capDeg)) + HALF_PIXEL);
    context.lineTo(widthPx, Math.round(yOf(share * run.capDeg)) + HALF_PIXEL);
    context.stroke();
  }
  context.setLineDash([]);
}

/** The distances per pixel column as a filled area down to no distance. */
function drawArea(
  context: CanvasRenderingContext2D,
  style: TimelineStyle,
  widthPx: number,
  yOf: ChartY,
  values: number[],
  alpha: number,
): void {
  context.fillStyle = style.off;
  context.globalAlpha = alpha;
  context.beginPath();
  context.moveTo(0, yOf(0));
  values.forEach((value, column) => {
    context.lineTo(column, yOf(value));
    context.lineTo(column + 1, yOf(value));
  });
  context.lineTo(widthPx, yOf(0));
  context.closePath();
  context.fill();
  context.globalAlpha = 1;
}

/**
 * The strip: per pixel column, the state most of its frames had. Then the bots' deaths across it.
 */
function drawStrip(
  context: CanvasRenderingContext2D,
  run: Timeline,
  widthPx: number,
  style: TimelineStyle,
  stripY: number,
): void {
  const fills: Record<TrackState, string> = {
    [TrackState.NoBot]: style.grid,
    [TrackState.On]: style.on,
    [TrackState.Off]: style.off,
    [TrackState.Switching]: style.switching,
  };
  columns(run, widthPx, (firstFrame, endFrame, column) => {
    const framesPerState = [0, 0, 0, 0];
    for (let frame = firstFrame; frame < endFrame; frame++) framesPerState[run.state[frame]]++;
    context.fillStyle = fills[framesPerState.indexOf(Math.max(...framesPerState)) as TrackState];
    context.fillRect(column, stripY, 1, style.strip);
  });
  context.fillStyle = style.death;
  for (const death of run.deaths) {
    const x = Math.floor((death / run.frameCount) * widthPx);
    context.fillRect(x, stripY - DEATH_RISE, DEATH_WIDTH, style.strip + DEATH_RISE);
  }
}

/** The scale's top and bottom, labelled at the chart's left edge. */
function drawScaleLabels(
  context: CanvasRenderingContext2D,
  run: Timeline,
  style: TimelineStyle,
  chartHeight: number,
): void {
  context.font = style.font;
  context.textBaseline = 'middle';
  for (const [text, labelY] of [
    [`${run.capDeg.toFixed(1)}° off`, TOP + LABEL_HEIGHT / 2],
    ['0°: on the bot', TOP + chartHeight - LABEL_HEIGHT / 2],
  ] as const) {
    context.fillStyle = style.labelBg;
    const labelWidth = context.measureText(text).width + 2 * LABEL_PAD;
    context.fillRect(0, labelY - LABEL_HEIGHT / 2, labelWidth, LABEL_HEIGHT);
    context.fillStyle = style.text;
    context.fillText(text, LABEL_PAD, labelY);
  }
  context.textBaseline = 'alphabetic';
}

/** The seconds along the bottom. */
function drawSeconds(
  context: CanvasRenderingContext2D,
  run: Timeline,
  widthPx: number,
  heightPx: number,
): void {
  const seconds = run.frameCount / run.fps;
  const step = seconds > LONG_RUN_SECONDS ? LONG_STEP_SECONDS : STEP_SECONDS;
  for (let second = 0; second <= seconds; second += step) {
    const x = (second / seconds) * widthPx;
    if (second === 0) context.textAlign = 'left';
    else context.textAlign = x > widthPx - RIGHT_EDGE ? 'right' : 'center';
    context.fillText(`${second} s`, Math.min(widthPx - 1, x), heightPx - LABEL_PAD);
  }
  context.textAlign = 'left';
}

/**
 * The chart: how far outside the bot's edge the crosshair was, per pixel column the furthest
 * (light) and the average (dark); under it a strip of on target, off target and switching (the
 * state most of the column's frames had), the bots' deaths, and the seconds. The canvas is widthPx
 * by heightPx CSS pixels.
 */
export function drawTimeline(
  context: CanvasRenderingContext2D,
  run: Timeline,
  widthPx: number,
  heightPx: number,
  style: TimelineStyle,
): void {
  const chartHeight = heightPx - TOP - STRIP_GAP - style.strip - AXIS;
  const stripY = TOP + chartHeight + STRIP_GAP;
  const yOf: ChartY = (distanceDeg) =>
    TOP + chartHeight * (1 - Math.min(distanceDeg, run.capDeg) / run.capDeg);
  context.clearRect(0, 0, widthPx, heightPx);
  drawGrid(context, run, widthPx, style, yOf);
  const distances = columnDistances(run, widthPx);
  drawArea(context, style, widthPx, yOf, distances.furthest, MAX_ALPHA);
  drawArea(context, style, widthPx, yOf, distances.mean, MEAN_ALPHA);
  drawStrip(context, run, widthPx, style, stripY);
  drawScaleLabels(context, run, style, chartHeight);
  drawSeconds(context, run, widthPx, heightPx);
}
