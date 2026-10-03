import { TrackReport, Tracks } from '../../api';
import { formatDegrees, formatPercent } from '../../format';
import { median } from '../median';
import {
  AxisTick,
  BOX,
  ChartBox,
  ChartPoint,
  niceStep,
  ticks,
} from '../run-charts/run-charts-model';
import { boxes, nearest, Timeline, TrackState } from '../track';

/** A stretch of the run as a bar: where it starts in the video, its place, and what it says on hover. */
export interface WindowBar {
  seconds: number;
  x: number;
  width: number;
  y: number;
  height: number;
  title: string;
}

/** The time on the bot in each stretch of the run, against the whole run's. */
export interface OnTargetModel {
  box: ChartBox;
  bars: WindowBar[];
  xTicks: AxisTick[];
  yTicks: AxisTick[];
  /** The whole run's time on the bot, as a line. */
  overall: AxisTick | null;
}

/** One band of distances, as a bar. */
export interface SpreadBar {
  x: number;
  width: number;
  y: number;
  height: number;
  title: string;
}

/** How the distance from the bot's center line was spread, with the bot's edge and the median marked. */
export interface SpreadModel {
  box: ChartBox;
  bars: SpreadBar[];
  xTicks: AxisTick[];
  yTicks: AxisTick[];
  edge: AxisTick | null;
  median: AxisTick | null;
}

/** A cell of the map: its place and size, its shade (0 to 1, by the time spent there), and what it says on hover. */
export interface MapCell {
  x: number;
  y: number;
  size: number;
  shade: number;
  title: string;
}

/**
 * Where the crosshair sat around the bot while it moved, the bot turned to move to the right: right of its center is
 * ahead of it, left behind, up and down to its sides. reason: why there is no map.
 */
export interface AroundModel {
  box: ChartBox;
  cells: MapCell[];
  center: ChartPoint;
  /** The bot's usual edge. */
  radius: number;
  /** The crosshair's usual place along the motion (the median). */
  usual: ChartPoint | null;
  xTicks: AxisTick[];
  reason: string | null;
}

/** The map's cells across its height. */
const MAP_ROWS = 24;
const OLD_REVIEW = 'The review of this run is older than this map: review it again to see it.';

/** The length of a stretch, in seconds; a last, shorter one is shown when it holds this much of the run. */
export const WINDOW = 10;
const LAST_WINDOW_MIN = 3;
/** A stretch with less tracking than this, in seconds, has no bar. */
const TRACKED_MIN = 2;
const SPREAD_BANDS = 30;

const plotWidth = (b: ChartBox) => b.width - b.left - b.right;
const plotBottom = (b: ChartBox) => b.height - b.bottom;
const plotHeight = (b: ChartBox) => plotBottom(b) - b.top;

/**
 * The share of the time on the bot in each 10 s of the run, of the time tracking (the timeline's on and off frames:
 * no bot on screen and switching after a death are left out), as the "On target" card counts it.
 */
export function onTargetWindows(report: TrackReport, tl: Timeline): OnTargetModel {
  const box = BOX;
  const per = Math.round(WINDOW * tl.fps);
  const runSeconds = tl.n / tl.fps;
  const x = (seconds: number) =>
    box.left + (plotWidth(box) * seconds) / Math.max(WINDOW, runSeconds);
  const y = (share: number) => plotBottom(box) - plotHeight(box) * share;
  const bars: WindowBar[] = [];
  for (let k0 = 0; k0 < tl.n; k0 += per) {
    const k1 = Math.min(tl.n, k0 + per);
    if (k1 - k0 < LAST_WINDOW_MIN * tl.fps) break;
    let on = 0;
    let off = 0;
    for (let k = k0; k < k1; k++) {
      if (tl.state[k] === TrackState.On) on++;
      else if (tl.state[k] === TrackState.Off) off++;
    }
    if (on + off < TRACKED_MIN * tl.fps) continue;
    const share = on / (on + off);
    const from = k0 / tl.fps;
    const to = k1 / tl.fps;
    const gap = 0.1 * (x(to) - x(from));
    bars.push({
      seconds: (tl.start + k0) / tl.fps,
      x: x(from) + gap / 2,
      width: x(to) - x(from) - gap,
      y: y(share),
      height: plotHeight(box) * share,
      title: `${Math.round(from)} to ${Math.round(to)} s: ${formatPercent(share)} on the bot`,
    });
  }
  const xTicks = ticks(Math.max(WINDOW, runSeconds), x, (s) => `${Math.round(s)} s`);
  const overall = report.summary.on_target;
  return {
    box,
    bars,
    xTicks,
    yTicks: ticks(1, y, (s) => formatPercent(s)),
    overall:
      overall == null ? null : { at: y(overall), label: `whole run ${formatPercent(overall)}` },
  };
}

/**
 * How far the crosshair was from the bot's center line (a capsule's long axis, a sphere's center), in the frames on
 * or near it (within three of its radii, as "Distance from the center" counts them): the share of those frames in
 * each band of distance, with the bot's usual edge and the median distance.
 */
export function distanceSpread(tracks: Tracks, tl: Timeline): SpreadModel {
  const box = BOX;
  const near: number[] = [];
  const radii: number[] = [];
  for (let k = 0; k < tl.n; k++) {
    if (tl.state[k] !== TrackState.On && tl.state[k] !== TrackState.Off) continue;
    const f = tracks.frames[tl.start + k];
    const best = f ? nearest(boxes(f)) : null;
    if (!best) continue;
    const r = Math.min(best.w, best.h) / 2;
    if (best.d > 3 * Math.max(r, 0.2)) continue;
    near.push(best.d);
    radii.push(r);
  }
  const r = median(radii);
  const sorted = [...near].sort((a, b) => a - b);
  // the scale reaches nearly every distance, and past the bot's edge
  const reach = Math.max(
    sorted.length ? sorted[Math.floor(0.98 * (sorted.length - 1))] : 1,
    1.25 * (r ?? 0),
  );
  const step = niceStep(reach) / 5;
  const bands = Math.min(SPREAD_BANDS, Math.max(1, Math.ceil(reach / step)));
  const top = bands * step;
  const counts = new Array<number>(bands).fill(0);
  for (const d of near) counts[Math.min(bands - 1, Math.floor(d / step))]++;
  const shares = counts.map((c) => c / Math.max(1, near.length));
  const highest = Math.max(0.05, ...shares) * 1.1;
  const x = (deg: number) => box.left + (plotWidth(box) * deg) / top;
  const y = (share: number) => plotBottom(box) - (plotHeight(box) * share) / highest;
  const bars = shares.map((share, i) => ({
    x: x(i * step) + 0.5,
    width: Math.max(1, x(step) - x(0) - 1),
    y: y(share),
    height: plotBottom(box) - y(share),
    title:
      i === bands - 1
        ? `${formatDegrees(i * step)} and over: ${formatPercent(share)} of the time`
        : `${formatDegrees(i * step)} to ${formatDegrees((i + 1) * step)}: ${formatPercent(share)} of the time`,
  }));
  const mid = median(near);
  return {
    box,
    bars,
    xTicks: ticks(top, x, (deg) => formatDegrees(deg, Number.isInteger(+deg.toFixed(6)) ? 0 : 1)),
    yTicks: ticks(highest, y, (s) => formatPercent(s)),
    edge: r == null || r > top ? null : { at: x(r), label: `its edge ${formatDegrees(r)}` },
    median: mid == null ? null : { at: x(mid), label: `median ${formatDegrees(mid)}` },
  };
}

/** Where the crosshair sat around the moving bot (the motion's frames, review.track_motion's "around"). */
export function aroundMap(report: TrackReport): AroundModel {
  const box = BOX;
  const motion = report.summary.motion;
  const pts = motion?.around ?? [];
  const center: ChartPoint = { x: box.left + plotWidth(box) / 2, y: box.top + plotHeight(box) / 2 };
  const empty = (reason: string): AroundModel => ({
    box,
    cells: [],
    center,
    radius: 0,
    usual: null,
    xTicks: [],
    reason,
  });
  if (!motion) return empty(OLD_REVIEW);
  if (motion.reason) return empty(`Not measured: ${motion.reason}.`);
  if (!pts.length) return empty(OLD_REVIEW);
  const r = median(pts.map((p) => p[2])) ?? 0;
  const across = pts.map((p) => Math.abs(p[1])).sort((a, b) => a - b);
  const reach = Math.max(1.5 * r, across[Math.floor(0.98 * (across.length - 1))]);
  const scale = plotHeight(box) / 2 / reach;
  const halfWidth = plotWidth(box) / 2 / scale;
  const cell = (2 * reach) / MAP_ROWS;
  const columns = Math.floor((2 * halfWidth) / cell);
  const counts = new Map<number, number>();
  for (const [along, side] of pts) {
    const col = Math.floor((along + (columns * cell) / 2) / cell);
    const row = Math.floor((side + reach) / cell);
    if (col < 0 || col >= columns || row < 0 || row >= MAP_ROWS) continue;
    const key = row * columns + col;
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  const most = Math.max(1, ...counts.values());
  const left = center.x - (columns * cell * scale) / 2;
  const cells = [...counts].map(([key, count]): MapCell => {
    const row = Math.floor(key / columns);
    const col = key % columns;
    const along = (col + 0.5) * cell - (columns * cell) / 2;
    const side = (row + 0.5) * cell - reach;
    const way = along >= 0 ? 'ahead' : 'behind';
    return {
      x: left + col * cell * scale,
      y: box.top + (MAP_ROWS - 1 - row) * cell * scale,
      size: cell * scale,
      shade: Math.sqrt(count / most),
      title: `${formatDegrees(Math.abs(along))} ${way}, ${formatDegrees(Math.abs(side))} to the side: ${formatPercent(count / pts.length)} of the time`,
    };
  });
  const usual = median(pts.map((p) => p[0]));
  const step = niceStep(halfWidth);
  const xTicks: AxisTick[] = [];
  for (let v = -Math.floor(halfWidth / step) * step; v <= halfWidth + 1e-9; v += step) {
    const label =
      Math.abs(v) < 1e-9
        ? '0'
        : `${v > 0 ? '+' : '−'}${formatDegrees(Math.abs(v), step < 1 ? 1 : 0)}`;
    xTicks.push({ at: center.x + v * scale, label });
  }
  return {
    box,
    cells,
    center,
    radius: r * scale,
    usual: usual == null ? null : { x: center.x + usual * scale, y: center.y },
    xTicks,
    reason: null,
  };
}
