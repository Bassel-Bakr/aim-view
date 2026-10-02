import { Flick, Geometry, TrackFrame, TrackReport, Tracks } from '../api';

/** A point on the canvas, in CSS pixels. */
export type Point = [x: number, y: number];

/** Degrees from the crosshair to canvas pixels, through the camera's projection. */
export function toPx(g: Geometry, xd: number, yd: number, scale: number): Point {
  const x = g.CX + g.K * Math.tan((xd * Math.PI) / 180);
  const y = g.CY - Math.tan((yd * Math.PI) / 180) * Math.hypot(g.K, x - g.CX);
  return [x * scale, y * scale];
}

/** The flick on screen at a frame: the latest one to have started by then. */
export function flickAt(flicks: Flick[], frame: number): Flick | null {
  let at: Flick | null = null;
  for (const f of flicks)
    if (f.start_frame <= frame && (!at || f.start_frame > at.start_frame)) at = f;
  return at;
}

/** A target's box in degrees, and how the crosshair stands to it. */
export interface TargetBox {
  x: number;
  y: number;
  w: number;
  h: number;
  /** From the crosshair to the target's center line (a capsule's long axis, a sphere's center). */
  d: number;
  /** The crosshair is on it. */
  inside: boolean;
  /** How far outside its edge the crosshair is (0 inside). */
  out: number;
}

const DEFAULT_SIZE = 0.6;
const EDGE = 0.05;

/** Each target in a frame as a box. */
export function boxes(f: TrackFrame): TargetBox[] {
  return f.t.map(([, x, y], k) => {
    const [w, h] = f.wh ? f.wh[k] : [DEFAULT_SIZE, DEFAULT_SIZE];
    const lx = Math.sign(x) * Math.max(0, Math.abs(x) - Math.max(0, w - h) / 2);
    const ly = Math.sign(y) * Math.max(0, Math.abs(y) - Math.max(0, h - w) / 2);
    const d = Math.hypot(lx, ly);
    const inside = Math.abs(x) <= w / 2 + EDGE && Math.abs(y) <= h / 2 + EDGE;
    return { x, y, w, h, d, inside, out: Math.max(0, d - Math.min(w, h) / 2) };
  });
}

/** The box nearest the crosshair, the bot the review follows. */
export function nearest(bs: TargetBox[]): TargetBox | null {
  let best: TargetBox | null = null;
  for (const b of bs) if (!best || b.d < best.d) best = b;
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
  n: number;
  fps: number;
  state: Int8Array;
  dist: Float32Array;
  cap: number;
  deaths: number[];
}

export function timeline(report: TrackReport, tracks: Tracks): Timeline {
  const s = report.summary;
  const start = s.start ?? 0;
  const end = Math.min(s.end ?? tracks.frames.length, tracks.frames.length);
  const n = Math.max(0, end - start);
  const state = new Int8Array(n);
  const dist = new Float32Array(n).fill(NaN);
  for (let i = start; i < end; i++) {
    const f = tracks.frames[i];
    if (!f) continue;
    const bs = boxes(f);
    const best = nearest(bs);
    const switching = s.switches.some(([a, b]) => i >= a && i < b);
    const inside = bs.some((b) => b.inside);
    state[i - start] = switching
      ? TrackState.Switching
      : inside
        ? TrackState.On
        : best
          ? TrackState.Off
          : TrackState.NoBot;
    if (best && !switching) dist[i - start] = inside ? 0 : best.out;
  }
  const off = [...dist].filter((v) => v > 0).sort((a, b) => a - b);
  const p98 = off.length ? off[Math.floor(0.98 * (off.length - 1))] : 0.5;
  const cap = Math.min(5, Math.max(0.5, p98));
  return {
    start,
    n,
    fps: report.fps,
    state,
    dist,
    cap,
    deaths: s.switches.map(([d]) => d - start),
  };
}

/** A clock: 75.25 s as "1:15.3". */
export function clock(seconds: number): string {
  return `${Math.floor(seconds / 60)}:${(seconds % 60).toFixed(1).padStart(4, '0')}`;
}

const STATE_TEXT = ['no bot seen', 'on target', 'off target', 'switching after a death'];

/** What the timeline says about a moment, for its tooltip and for screen readers. */
export function describe(tl: Timeline, k: number): string {
  const d = tl.dist[k];
  const away = Number.isNaN(d) || d === 0 ? '' : ` · ${d.toFixed(2)}° outside its edge`;
  return `${clock((tl.start + k) / tl.fps)} · ${STATE_TEXT[tl.state[k]]}${away}`;
}
