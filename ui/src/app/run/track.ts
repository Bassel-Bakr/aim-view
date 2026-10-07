/**
 * The geometry the run page draws with: degrees from the crosshair to canvas pixels, the flick on
 * screen, each target's box and shape, and a tracking run's timeline, frame by frame. In: the
 * review's report (geometry, hitbox, flicks, summary) and its tracks (tracks.json). Out: the video
 * overlay, the timeline, the track charts, the faint-target overlay, the player's clock and
 * FlickFocus.
 */

import { Flick, Geometry, Hitbox, TrackFrame, TrackReport, Tracks } from '../api';

/** A point on the canvas, in CSS pixels. */
export type Point = [x: number, y: number];

/**
 * Degrees from the crosshair (right and up positive) to canvas pixels, through the camera's
 * projection; `scale` is canvas CSS pixels per pixel of the frame the geometry describes.
 */
export function toPx(geometry: Geometry, xDeg: number, yDeg: number, scale: number): Point {
  const x = geometry.CX + geometry.K * Math.tan((xDeg * Math.PI) / 180);
  const y =
    geometry.CY - Math.tan((yDeg * Math.PI) / 180) * Math.hypot(geometry.K, x - geometry.CX);
  return [x * scale, y * scale];
}

/**
 * The flick on screen at a frame: the latest one to have started by then; null before the first.
 */
export function flickAt(flicks: Flick[], frame: number): Flick | null {
  let at: Flick | null = null;
  for (const flick of flicks)
    if (flick.start_frame <= frame && (!at || flick.start_frame > at.start_frame)) at = flick;
  return at;
}

/** A target's shape on screen: an ellipse (a spheroid's), an upright capsule, or a box. */
export type TargetShapeKind = 'ellipse' | 'capsule' | 'box';

/** A target's shape on screen, centered on its box: its kind and its half sides in degrees. */
export interface TargetShape {
  /** Which outline the target has. */
  kind: TargetShapeKind;
  /** Half the shape's width, in degrees. */
  halfWidthDeg: number;
  /** Half the shape's height, in degrees. */
  halfHeightDeg: number;
}

/** A target's box in degrees, its shape, and how the crosshair stands to it. */
export interface TargetBox {
  /** The box's center, in degrees right of the crosshair. */
  x: number;
  /** The box's center, in degrees up from the crosshair. */
  y: number;
  /** The box's width, in degrees. */
  widthDeg: number;
  /** The box's height, in degrees. */
  heightDeg: number;
  /** The target's outline in the box, from the bots' hitbox. */
  shape: TargetShape;
  /** From the crosshair to the target's center line (a capsule's long axis, a sphere's center). */
  centerLineDeg: number;
  /** The crosshair is on it. */
  inside: boolean;
  /** How far outside its edge the crosshair is (0 inside). */
  outsideDeg: number;
}

/** A target's width and height where the model gave no size, in degrees. */
const DEFAULT_SIZE_DEG = 0.6;
/** How far past a target's edge the crosshair still counts as on it, in degrees. */
const EDGE_DEG = 0.05;

/**
 * A target's shape from its box and the bots' hitbox (the report's; null: a plain box), sized as
 * the core's on-target test sizes it (src/tracking.rs: `on_box`): the box's side along the hitbox's
 * longer axis, the other side from the hitbox's width over its height, which stays as the bot comes
 * near or goes far.
 */
export function targetShape(
  widthDeg: number,
  heightDeg: number,
  hitbox: Hitbox | null,
): TargetShape {
  if (!hitbox) return { kind: 'box', halfWidthDeg: widthDeg / 2, halfHeightDeg: heightDeg / 2 };
  const ratio = hitbox.widthToHeight;
  const tall = ratio < 1 ? heightDeg : ratio > 1 ? widthDeg / ratio : Math.max(widthDeg, heightDeg);
  const halfWidthDeg = (tall * ratio) / 2;
  const halfHeightDeg = tall / 2;
  const capsule = hitbox.kind === 'cylindrical' && halfHeightDeg > halfWidthDeg;
  const kind = hitbox.kind === 'cuboid' ? 'box' : capsule ? 'capsule' : 'ellipse';
  return { kind, halfWidthDeg, halfHeightDeg };
}

/**
 * Whether the crosshair (the origin) is on a target centered at (x, y) degrees with this shape, or
 * within EDGE_DEG of it.
 */
export function onShape(x: number, y: number, shape: TargetShape): boolean {
  const halfWidth = shape.halfWidthDeg + EDGE_DEG;
  const halfHeight = shape.halfHeightDeg + EDGE_DEG;
  if (shape.kind === 'box') return Math.abs(x) <= halfWidth && Math.abs(y) <= halfHeight;
  if (shape.kind === 'capsule')
    return Math.hypot(x, Math.max(0, Math.abs(y) - (halfHeight - halfWidth))) <= halfWidth;
  return (x / halfWidth) ** 2 + (y / halfHeight) ** 2 <= 1;
}

/** Each target in a frame as a box, in the bots' hitbox shape when the report has one. */
export function boxes(frame: TrackFrame, hitbox: Hitbox | null = null): TargetBox[] {
  return frame.t.map(([, x, y], targetIndex) => {
    const [widthDeg, heightDeg] = frame.wh
      ? frame.wh[targetIndex]
      : [DEFAULT_SIZE_DEG, DEFAULT_SIZE_DEG];
    const axisX = Math.sign(x) * Math.max(0, Math.abs(x) - Math.max(0, widthDeg - heightDeg) / 2);
    const axisY = Math.sign(y) * Math.max(0, Math.abs(y) - Math.max(0, heightDeg - widthDeg) / 2);
    const centerLineDeg = Math.hypot(axisX, axisY);
    const shape = targetShape(widthDeg, heightDeg, hitbox);
    const inside = onShape(x, y, shape);
    const outsideDeg = Math.max(0, centerLineDeg - Math.min(widthDeg, heightDeg) / 2);
    return { x, y, widthDeg, heightDeg, shape, centerLineDeg, inside, outsideDeg };
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
  /** No bot was found in the frame. */
  NoBot = 0,
  /** The crosshair is on a bot. */
  On = 1,
  /** A bot shows, and the crosshair is off it. */
  Off = 2,
  /** Between a bot's death and the crosshair being back on a target (the summary's switches). */
  Switching = 3,
}

/**
 * A tracking run, moment by moment: the state of each frame in the run and how far outside the
 * bot's edge the crosshair was (NaN where no bot was seen or the run was switching), the scale's
 * top (cap, degrees) and the frames where bots died. Frames count from start.
 */
export interface Timeline {
  /** The run's first frame in the recording (the summary's start, else 0). */
  start: number;
  /** How many frames the run has, from start to the run's end. */
  frameCount: number;
  /** The recording's frames a second. */
  fps: number;
  /** Each frame's TrackState. */
  state: Int8Array;
  /**
   * Each frame's distance from the crosshair to the nearest bot's edge, in degrees: 0 on a bot,
   * NaN where no bot was seen or the run was switching.
   */
  outsideDeg: Float32Array;
  /** The scale's top for the distances, in degrees. */
  capDeg: number;
  /** The frames where bots died, counted from start. */
  deaths: number[];
}

/** The scale's top is this percentile of the distances off target. */
const CAP_PERCENTILE = 0.98;
/** The lowest the scale's top goes, in degrees. */
const MIN_CAP_DEG = 0.5;
/** The highest the scale's top goes, in degrees. */
const MAX_CAP_DEG = 5;

/**
 * A tracking run's timeline from its report and tracks: each frame's state and distance off the
 * nearest bot, between the summary's start and end (the tracks' end when it has none).
 */
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
    const targetBoxes = boxes(trackFrame, report.hitbox ?? null);
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

/** Each TrackState in words, in the enum's order. */
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
