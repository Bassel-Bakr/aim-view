import { Flick, PathPoint } from '../../api';
import { SpeedPoint, speeds } from '../speed';

/** The chart's size in pixels. */
export interface ChartSize {
  width: number;
  height: number;
}

/** A horizontal grid line at a speed, with its label. */
export interface GridLine {
  y: number;
  label: string;
}

/** A label on the time axis. */
export interface TimeTick {
  x: number;
  label: string;
}

/** A moment of the flick (moving, flick done, on target, click), as a dashed line with its name. */
export interface FlickMoment {
  x: number;
  label: string;
}

/** A moment's name and its time into the flick, in seconds (null when it was not found). */
type MomentTime = [label: string, seconds: number | null];

/** A flick's speed chart, laid out in pixels. topSpeed: the speed at the top of the chart. */
export interface SpeedChartModel {
  size: ChartSize;
  topSpeed: number;
  top: number;
  bottom: number;
  left: number;
  right: number;
  grid: GridLine[];
  ticks: TimeTick[];
  moments: FlickMoment[];
  line: string;
  data: SpeedPoint[];
  firstFrame: number;
  lastFrame: number;
  fps: number;
}

const MARGIN_LEFT = 40;
const MARGIN_RIGHT = 10;
const MARGIN_TOP = 8;
const MARGIN_BOTTOM = 22;
/** The speed axis reaches at least this far, in degrees per second, and 8% past the fastest point. */
const MIN_TOP_SPEED = 60;
const HEADROOM = 1.08;
/** The time axis is marked every 100 ms, or every 200 ms on a flick longer than 600 ms. */
const TICK_MS = 100;
const LONG_TICK_MS = 200;
const LONG_FLICK_MS = 600;

export function xOf(model: SpeedChartModel, frame: number): number {
  const span = Math.max(1, model.lastFrame - model.firstFrame);
  return (
    model.left + ((model.size.width - model.left - model.right) * (frame - model.firstFrame)) / span
  );
}

/** The frame at a pixel across the chart. */
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

/** The chart of a flick's crosshair speed, from the start of the flick to its kill. */
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

/** The y of a speed on the chart. */
export function yOf(model: SpeedChartModel, speed: number): number {
  return model.bottom - ((model.bottom - model.top) * speed) / model.topSpeed;
}
