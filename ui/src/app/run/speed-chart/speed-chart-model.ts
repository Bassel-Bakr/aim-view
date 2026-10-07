/**
 * The speed chart's layout for one flick.
 *
 * In: the flick, its target's path relative to the crosshair (the report's `paths`), the frame rate
 * and the chart's size on screen.
 * Out: the grid, ticks, moments and speed line in the SVG's pixels (speed-chart.html draws them),
 * and the conversions between frames and pixels across.
 */

import { Flick, PathPoint } from '../../api';
import { SpeedPoint, speeds } from '../speed';

/** The chart's size in pixels. */
export interface ChartSize {
  /** The chart's width, in pixels. */
  width: number;
  /** The chart's height, in pixels. */
  height: number;
}

/** A horizontal grid line at a speed, with its label. */
export interface GridLine {
  /** The line's height on the chart, in pixels from the top. */
  y: number;
  /** The speed, in degrees a second. */
  label: string;
}

/** A label on the time axis. */
export interface TimeTick {
  /** The tick's place across, in pixels. */
  x: number;
  /** The time from the flick's start ("200 ms"). */
  label: string;
}

/** A moment of the flick (moving, flick done, on target, click), as a dashed line with its name. */
export interface FlickMoment {
  /** The moment's place across, in pixels. */
  x: number;
  /** The moment's name. */
  label: string;
}

/** A moment's name and its time into the flick, in seconds (null when it was not found). */
type MomentTime = [label: string, seconds: number | null];

/** A flick's speed chart, laid out in pixels. topSpeed: the speed at the top of the chart. */
export interface SpeedChartModel {
  /** The chart's size on screen. */
  size: ChartSize;
  /** The speed at the plot's top, in degrees a second. */
  topSpeed: number;
  /** The plot's top, in pixels from the chart's top. */
  top: number;
  /** The plot's bottom (no speed), in pixels from the chart's top. */
  bottom: number;
  /** The plot's left edge, in pixels: room for the speed labels. */
  left: number;
  /** The margin right of the plot, in pixels. */
  right: number;
  /** The speed axis's grid lines. */
  grid: GridLine[];
  /** The time axis's labels. */
  ticks: TimeTick[];
  /** The flick's moments that were found. */
  moments: FlickMoment[];
  /** The speed line, as an SVG path. */
  line: string;
  /** The speeds the line goes through, one a frame. */
  data: SpeedPoint[];
  /** The flick's first frame: the plot's left edge. */
  firstFrame: number;
  /** The flick's kill frame: the plot's right edge. */
  lastFrame: number;
  /** The recording's frames a second. */
  fps: number;
}

/** The plot's left margin, for the speed labels, in pixels. */
const MARGIN_LEFT = 40;
/** The plot's right margin, in pixels. */
const MARGIN_RIGHT = 10;
/** The plot's top margin, in pixels. */
const MARGIN_TOP = 8;
/** The plot's bottom margin, for the time labels, in pixels. */
const MARGIN_BOTTOM = 22;
/** The speed axis reaches at least this far, in degrees per second. */
const MIN_TOP_SPEED = 60;
/** The speed axis reaches 8% past the fastest point, so the line's peak stays off the edge. */
const HEADROOM = 1.08;
/** Milliseconds between the time axis's marks. */
const TICK_MS = 100;
/** Milliseconds between the time axis's marks on a flick longer than `LONG_FLICK_MS`. */
const LONG_TICK_MS = 200;
/** A flick longer than this, in milliseconds, gets the longer step between marks. */
const LONG_FLICK_MS = 600;

/** The pixel across the chart at a frame of the recording (frames may be fractional). */
export function xOf(model: SpeedChartModel, frame: number): number {
  const span = Math.max(1, model.lastFrame - model.firstFrame);
  return (
    model.left + ((model.size.width - model.left - model.right) * (frame - model.firstFrame)) / span
  );
}

/** The frame of the recording at a pixel across the chart, fractional: the inverse of `xOf`. */
export function frameAt(model: SpeedChartModel, x: number): number {
  const span = model.lastFrame - model.firstFrame;
  return (
    model.firstFrame + ((x - model.left) / (model.size.width - model.left - model.right)) * span
  );
}

/** The time from the flick's start, in milliseconds: every 100 ms, or 200 ms past 600 ms. */
function timeTicks(model: SpeedChartModel): TimeTick[] {
  const ms = Math.round((1000 * (model.lastFrame - model.firstFrame)) / model.fps);
  const tickStep = ms > LONG_FLICK_MS ? LONG_TICK_MS : TICK_MS;
  const ticks: TimeTick[] = [];
  for (let tickMs = 0; tickMs <= ms; tickMs += tickStep) {
    const frame = model.firstFrame + (tickMs * model.fps) / 1000;
    ticks.push({ x: xOf(model, frame), label: `${tickMs} ms` });
  }
  return ticks;
}

/** The flick's moments that were found: moving, flick done, on target and the click. */
function flickMoments(model: SpeedChartModel, flick: Flick): FlickMoment[] {
  const flickDone = flick.react != null && flick.flick != null ? flick.react + flick.flick : null;
  const moments: MomentTime[] = [
    ['moving', flick.react],
    ['flick done', flickDone],
    ['on target', flick.arrive],
    ['click', flick.total],
  ];
  return moments.flatMap(([label, seconds]) =>
    seconds == null ? [] : [{ x: xOf(model, model.firstFrame + seconds * model.fps), label }],
  );
}

/**
 * The chart of a flick's crosshair speed, from the start of the flick to its kill. Grid lines come
 * every 20, 50 or 100 degrees a second, by the chart's top speed. `smooth` smooths the speeds
 * (speed.ts `speeds`).
 */
export function speedChart(
  flick: Flick,
  path: PathPoint[],
  fps: number,
  size: ChartSize,
  smooth: boolean,
): SpeedChartModel {
  const data = speeds(path, fps, smooth);
  const top = Math.max(MIN_TOP_SPEED, ...data.map(([, speed]) => speed)) * HEADROOM;
  const model: SpeedChartModel = {
    size,
    topSpeed: top,
    top: MARGIN_TOP,
    bottom: size.height - MARGIN_BOTTOM,
    left: MARGIN_LEFT,
    right: MARGIN_RIGHT,
    grid: [],
    ticks: [],
    moments: [],
    line: '',
    data,
    firstFrame: flick.start_frame,
    lastFrame: flick.kill_frame,
    fps,
  };
  const y = (speed: number) => yOf(model, speed);
  const step = top > 300 ? 100 : top > 120 ? 50 : 20;
  for (let speed = 0; speed <= top; speed += step)
    model.grid.push({ y: y(speed), label: String(speed) });
  model.ticks.push(...timeTicks(model));
  model.moments.push(...flickMoments(model, flick));
  model.line = data
    .map(
      ([frame, speed], i) =>
        `${i ? 'L' : 'M'}${xOf(model, frame).toFixed(1)},${y(speed).toFixed(1)}`,
    )
    .join('');
  return model;
}

/** The height on the chart of a speed in degrees a second, in pixels from the top. */
export function yOf(model: SpeedChartModel, speed: number): number {
  return model.bottom - ((model.bottom - model.top) * speed) / model.topSpeed;
}
