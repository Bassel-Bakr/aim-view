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

export function xOf(m: SpeedChartModel, frame: number): number {
  const span = Math.max(1, m.lastFrame - m.firstFrame);
  return m.left + ((m.size.width - m.left - m.right) * (frame - m.firstFrame)) / span;
}

/** The frame at a pixel across the chart. */
export function frameAt(m: SpeedChartModel, x: number): number {
  const span = m.lastFrame - m.firstFrame;
  return m.firstFrame + ((x - m.left) / (m.size.width - m.left - m.right)) * span;
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
  const top = Math.max(MIN_TOP_SPEED, ...data.map(([, v]) => v)) * HEADROOM;
  const m: SpeedChartModel = {
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
  const y = (v: number) => yOf(m, v);
  const step = top > 300 ? 100 : top > 120 ? 50 : 20;
  for (let v = 0; v <= top; v += step) m.grid.push({ y: y(v), label: String(v) });
  const ms = Math.round((1000 * (m.lastFrame - m.firstFrame)) / fps);
  const tickStep = ms > 600 ? 200 : 100;
  for (let t = 0; t <= ms; t += tickStep) {
    m.ticks.push({ x: xOf(m, m.firstFrame + (t * fps) / 1000), label: `${t} ms` });
  }
  const flickDone = flick.react != null && flick.flick != null ? flick.react + flick.flick : null;
  const moments: MomentTime[] = [
    ['moving', flick.react],
    ['flick done', flickDone],
    ['on target', flick.arrive],
    ['click', flick.total],
  ];
  for (const [label, t] of moments) {
    if (t != null) m.moments.push({ x: xOf(m, m.firstFrame + t * fps), label });
  }
  m.line = data
    .map(([f, v], i) => `${i ? 'L' : 'M'}${xOf(m, f).toFixed(1)},${y(v).toFixed(1)}`)
    .join('');
  return m;
}

/** The y of a speed on the chart. */
export function yOf(m: SpeedChartModel, speed: number): number {
  return m.bottom - ((m.bottom - m.top) * speed) / m.topSpeed;
}
