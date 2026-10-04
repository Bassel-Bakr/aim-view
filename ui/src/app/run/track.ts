import { Flick, Geometry, TrackFrame, TrackReport, Tracks } from '../api';

/** A point on the canvas, in CSS pixels. */
export type Point = [x: number, y: number];

/** Degrees from the crosshair to canvas pixels, through the camera's projection. */
export function toPx(geometry: Geometry, xDeg: number, yDeg: number, scale: number): Point {
  const x = geometry.CX + geometry.K * Math.tan((xDeg * Math.PI) / 180);
  const y =
    geometry.CY - Math.tan((yDeg * Math.PI) / 180) * Math.hypot(geometry.K, x - geometry.CX);
  return [x * scale, y * scale];
}

/** The flick on screen at a frame: the latest one to have started by then. */
export function flickAt(flicks: Flick[], frame: number): Flick | null {
  let at: Flick | null = null;
  for (const flick of flicks)
    if (flick.start_frame <= frame && (!at || flick.start_frame > at.start_frame)) at = flick;
  return at;
}

/** A target's box in degrees, and how the crosshair stands to it. */
export interface TargetBox {
  x: number;
  y: number;
  widthDeg: number;
  heightDeg: number;
  /** From the crosshair to the target's center line (a capsule's long axis, a sphere's center). */
  centerLineDeg: number;
  /** The crosshair is on it. */
  inside: boolean;
  /** How far outside its edge the crosshair is (0 inside). */
  outsideDeg: number;
}

/** A target's box where the model gave no size, and how far past its edge the crosshair still counts as on it. */
const DEFAULT_SIZE_DEG = 0.6;
const EDGE_DEG = 0.05;

/** Each target in a frame as a box. */
export function boxes(frame: TrackFrame): TargetBox[] {
  return frame.t.map(([, x, y], targetIndex) => {
    const [widthDeg, heightDeg] = frame.wh
      ? frame.wh[targetIndex]
      : [DEFAULT_SIZE_DEG, DEFAULT_SIZE_DEG];
    const axisX = Math.sign(x) * Math.max(0, Math.abs(x) - Math.max(0, widthDeg - heightDeg) / 2);
    const axisY = Math.sign(y) * Math.max(0, Math.abs(y) - Math.max(0, heightDeg - widthDeg) / 2);
    const centerLineDeg = Math.hypot(axisX, axisY);
    const inside =
      Math.abs(x) <= widthDeg / 2 + EDGE_DEG && Math.abs(y) <= heightDeg / 2 + EDGE_DEG;
    const outsideDeg = Math.max(0, centerLineDeg - Math.min(widthDeg, heightDeg) / 2);
    return { x, y, widthDeg, heightDeg, centerLineDeg, inside, outsideDeg };
  });
}

/** The box nearest the crosshair, the bot the review follows. */
export function nearest(targetBoxes: TargetBox[]): TargetBox | null {
  let best: TargetBox | null = null;
  for (const box of targetBoxes) if (!best || box.centerLineDeg < best.centerLineDeg) best = box;
  return best;
}

/** A frame of a tracking run, as the timeline's strip shows it. */
export enum TrackState {
  NoBot = 0,
  On = 1,
  Off = 2,
  Switching = 3,
}

/**
 * A tracking run, moment by moment: the state of each frame in the run and how far outside the bot's edge the
 * crosshair was (NaN where no bot was seen or the run was switching), the scale's top (cap, degrees) and the frames
 * where bots died. Frames count from start.
 */
export interface Timeline {
  start: number;
  frameCount: number;
  fps: number;
  state: Int8Array;
  outsideDeg: Float32Array;
  capDeg: number;
  deaths: number[];
}

/** The scale's top: the 98th percentile of the distances off target, kept between half a degree and 5 degrees. */
const CAP_PERCENTILE = 0.98;
const MIN_CAP_DEG = 0.5;
const MAX_CAP_DEG = 5;

export function timeline(report: TrackReport, tracks: Tracks): Timeline {
  const summary = report.summary;
  const start = summary.start ?? 0;
  const end = Math.min(summary.end ?? tracks.frames.length, tracks.frames.length);
  const frameCount = Math.max(0, end - start);
  const state = new Int8Array(frameCount);
  const outsideDeg = new Float32Array(frameCount).fill(NaN);
  for (let i = start; i < end; i++) {
    const trackFrame = tracks.frames[i];
    if (!trackFrame) continue;
    const targetBoxes = boxes(trackFrame);
    const best = nearest(targetBoxes);
    const switching = summary.switches.some(([a, b]) => i >= a && i < b);
    const inside = targetBoxes.some((box) => box.inside);
    state[i - start] = switching
      ? TrackState.Switching
      : inside
        ? TrackState.On
        : best
          ? TrackState.Off
          : TrackState.NoBot;
    if (best && !switching) outsideDeg[i - start] = inside ? 0 : best.outsideDeg;
  }
  const off = [...outsideDeg].filter((distance) => distance > 0).sort((a, b) => a - b);
  const percentile98 = off.length
    ? off[Math.floor(CAP_PERCENTILE * (off.length - 1))]
    : MIN_CAP_DEG;
  const capDeg = Math.min(MAX_CAP_DEG, Math.max(MIN_CAP_DEG, percentile98));
  return {
    start,
    frameCount,
    fps: report.fps,
    state,
    outsideDeg,
    capDeg,
    deaths: summary.switches.map(([death]) => death - start),
  };
}

/** A clock: 75.25 s as "1:15.3". */
export function clock(seconds: number): string {
  return `${Math.floor(seconds / 60)}:${(seconds % 60).toFixed(1).padStart(4, '0')}`;
}

const STATE_TEXT = ['no bot seen', 'on target', 'off target', 'switching after a death'];

/** What the timeline says about a moment, for its tooltip and for screen readers. */
export function describe(run: Timeline, moment: number): string {
  const outsideDeg = run.outsideDeg[moment];
  const away =
    Number.isNaN(outsideDeg) || outsideDeg === 0
      ? ''
      : ` · ${outsideDeg.toFixed(2)}° outside its edge`;
  return `${clock((run.start + moment) / run.fps)} · ${STATE_TEXT[run.state[moment]]}${away}`;
}
