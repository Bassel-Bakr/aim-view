/**
 * The shapes of the four charts in "The run at a glance" for a tracking run.
 *
 * In: the tracking report's summary (its time on target, and the motion's `around` points and
 * `turns_back`), the review's tracks, and the run moment by moment (track.ts `timeline`).
 * Out: bars, cells, dots and ticks in the run charts' SVG box (run-charts-model.ts `BOX`), which
 * track-charts.html draws.
 */

import { AroundPoint, TrackReport, Tracks } from '../../api';
import { formatDegrees, formatMs, formatPercent, formatSeconds } from '../../format';
import { median } from '../median';
import {
  AxisTick,
  BOX,
  ChartBox,
  ChartPoint,
  niceStep,
  plotBottom,
  plotHeight,
  plotWidth,
  ROUNDING_SLACK,
  ticks,
} from '../run-charts/run-charts-model';
import { boxes, nearest, Timeline, TrackState } from '../track';

/**
 * A stretch of the run as a bar: where it starts in the video, its place, and what it says on
 * hover.
 */
export interface WindowBar {
  /** Where the stretch starts in the video, in seconds: a click goes there. */
  seconds: number;
  /** The bar's left edge, in the chart's units. */
  x: number;
  /** The bar's width, in the chart's units. */
  width: number;
  /** The bar's top, in the chart's units. */
  y: number;
  /** The bar's height, in the chart's units. */
  height: number;
  /** The stretch's seconds into the run and its share on the bot, on hover. */
  title: string;
}

/** The time on the bot in each stretch of the run, against the whole run's. */
export interface OnTargetModel {
  /** The chart's box. */
  box: ChartBox;
  /** One bar for each stretch with enough tracking. */
  bars: WindowBar[];
  /** The seconds into the run. */
  xTicks: AxisTick[];
  /** The share on the bot, 0% to 100%. */
  yTicks: AxisTick[];
  /** The whole run's time on the bot, as a line. */
  overall: AxisTick | null;
}

/** One band of distances, as a bar. */
export interface SpreadBar {
  /** The bar's left edge, in the chart's units. */
  x: number;
  /** The bar's width, in the chart's units. */
  width: number;
  /** The bar's top, in the chart's units. */
  y: number;
  /** The bar's height, in the chart's units. */
  height: number;
  /** The band's distances and its share of the time, on hover. */
  title: string;
}

/**
 * How the distance from the bot's center line was spread, with the bot's edge and the median
 * marked.
 */
export interface SpreadModel {
  /** The chart's box. */
  box: ChartBox;
  /** One bar for each band of distance, nearest first. */
  bars: SpreadBar[];
  /** The distances, in degrees. */
  xTicks: AxisTick[];
  /** The share of the frames. */
  yTicks: AxisTick[];
  /** The bot's usual edge (its median radius); null when unknown or past the scale. */
  edge: AxisTick | null;
  /** The median distance; null with no frames near the bot. */
  median: AxisTick | null;
}

/**
 * A cell of the map: its place and size, its shade (0 to 1, by the time spent there), and what it
 * says on hover.
 */
export interface MapCell {
  /** The cell's left edge, in the chart's units. */
  x: number;
  /** The cell's top, in the chart's units. */
  y: number;
  /** The cell's side, in the chart's units. */
  size: number;
  /** 0 to 1: the square root of its frames against the fullest cell's. */
  shade: number;
  /** Where the cell is from the bot's center and its share of the time, on hover. */
  title: string;
}

/**
 * Where the crosshair sat around the bot while it moved, the bot turned to move to the right: right
 * of its center is ahead of it, left behind, up and down to its sides. reason: why there is no map.
 */
export interface AroundModel {
  /** The chart's box. */
  box: ChartBox;
  /** The cells the crosshair sat in. */
  cells: MapCell[];
  /** The bot's center, in the middle of the plot. */
  center: ChartPoint;
  /** The bot's usual edge. */
  radius: number;
  /** The crosshair's usual place along the motion (the median). */
  usual: ChartPoint | null;
  /** The degrees behind and ahead of the bot. */
  xTicks: AxisTick[];
  /** Why there is no map (an older review, or the motion was not measured); null with a map. */
  reason: string | null;
}

/**
 * A turn of the bot as a dot: where it starts in the video, its place, whether the crosshair was
 * not back on the bot before the next turn or the run's end (drawn at the top), and what it says on
 * hover.
 */
export interface TurnDot {
  /** Where the turn is in the video, in seconds: a click goes there. */
  seconds: number;
  /** The dot's place across: its time in the run. */
  x: number;
  /** The dot's place down: how long the crosshair took to get back on the bot. */
  y: number;
  /** The crosshair was not back on the bot before the next turn or the run's end. */
  lost: boolean;
  /** The turn's time and how long the crosshair took, on hover. */
  title: string;
}

/**
 * How long the crosshair took to get back on the bot after each of its turns, over the run, with
 * the median and a line that sums it up. reason: why there is no chart.
 */
export interface TurnsBackModel {
  /** The chart's box. */
  box: ChartBox;
  /** One dot for each turn. */
  dots: TurnDot[];
  /** The seconds into the run. */
  xTicks: AxisTick[];
  /** The time back on the bot. */
  yTicks: AxisTick[];
  /** The median time back on the bot, over the turns it came back from; null with none. */
  median: AxisTick | null;
  /** The line under the chart: the median, the share of quick turns and those lost. */
  note: string;
  /** Why there is no chart (an older review, not measured, or no turns); null with a chart. */
  reason: string | null;
}

/** A turn the crosshair was back on the bot within this many seconds counts as a quick one. */
export const QUICK_BACK = 0.2;
/** The turns chart's height reaches at least this many seconds. */
const TURNS_TOP_MIN = 0.5;

/** The map's cells across its height. */
const MAP_ROWS = 24;
/** The around map's note when the report has no `around` points: a review before they existed. */
const OLD_REVIEW = 'The review of this run is older than this map: review it again to see it.';
/** The turns chart's note when the report has no `turns_back`: a review before they existed. */
const OLD_REVIEW_CHART =
  'The review of this run is older than this chart: review it again to see it.';

/** The length of a stretch, in seconds. */
export const WINDOW = 10;
/** A last, shorter stretch is shown when it holds this many seconds of the run. */
const LAST_WINDOW_MIN = 3;
/** A stretch with less tracking than this, in seconds, has no bar. */
const TRACKED_MIN = 2;
/** The most bands of distance the spread chart has. */
const SPREAD_BANDS = 30;

/**
 * The share of the time on the bot in each 10 s of the run, of the time tracking (the timeline's on
 * and off frames: no bot on screen and switching after a death are left out), as the "On target"
 * card counts it.
 */
export function onTargetWindows(report: TrackReport, run: Timeline): OnTargetModel {
  const box = BOX;
  const framesPerWindow = Math.round(WINDOW * run.fps);
  const runSeconds = run.frameCount / run.fps;
  const x = (seconds: number) =>
    box.left + (plotWidth(box) * seconds) / Math.max(WINDOW, runSeconds);
  const y = (share: number) => plotBottom(box) - plotHeight(box) * share;
  const bars: WindowBar[] = [];
  for (let firstFrame = 0; firstFrame < run.frameCount; firstFrame += framesPerWindow) {
    const endFrame = Math.min(run.frameCount, firstFrame + framesPerWindow);
    if (endFrame - firstFrame < LAST_WINDOW_MIN * run.fps) break;
    let on = 0;
    let off = 0;
    for (let frame = firstFrame; frame < endFrame; frame++) {
      if (run.state[frame] === TrackState.On) on++;
      else if (run.state[frame] === TrackState.Off) off++;
    }
    if (on + off < TRACKED_MIN * run.fps) continue;
    const share = on / (on + off);
    const from = firstFrame / run.fps;
    const to = endFrame / run.fps;
    const gap = 0.1 * (x(to) - x(from));
    bars.push({
      seconds: (run.start + firstFrame) / run.fps,
      x: x(from) + gap / 2,
      width: x(to) - x(from) - gap,
      y: y(share),
      height: plotHeight(box) * share,
      title: `${Math.round(from)} to ${Math.round(to)} s: ${formatPercent(share)} on the bot`,
    });
  }
  const xTicks = ticks(Math.max(WINDOW, runSeconds), x, (seconds) => `${Math.round(seconds)} s`);
  const overall = report.summary.on_target;
  return {
    box,
    bars,
    xTicks,
    yTicks: ticks(1, y, (share) => formatPercent(share)),
    overall:
      overall == null ? null : { at: y(overall), label: `whole run ${formatPercent(overall)}` },
  };
}

/**
 * How far the crosshair was from the bot's center line (a capsule's long axis, a sphere's center),
 * in the frames on or near it (within three of its radii, as "Distance from the center" counts
 * them): the share of those frames in each band of distance, with the bot's usual edge and the
 * median distance.
 */
export function distanceSpread(tracks: Tracks, run: Timeline): SpreadModel {
  const box = BOX;
  const near: number[] = [];
  const radii: number[] = [];
  for (let frame = 0; frame < run.frameCount; frame++) {
    if (run.state[frame] !== TrackState.On && run.state[frame] !== TrackState.Off) continue;
    const trackFrame = tracks.frames[run.start + frame];
    const best = trackFrame ? nearest(boxes(trackFrame)) : null;
    if (!best) continue;
    const radius = Math.min(best.widthDeg, best.heightDeg) / 2;
    if (best.centerLineDeg > 3 * Math.max(radius, 0.2)) continue;
    near.push(best.centerLineDeg);
    radii.push(radius);
  }
  const usualRadius = median(radii);
  const sorted = [...near].sort((a, b) => a - b);
  // the scale reaches nearly every distance, and past the bot's edge
  const reach = Math.max(
    sorted.length ? sorted[Math.floor(0.98 * (sorted.length - 1))] : 1,
    1.25 * (usualRadius ?? 0),
  );
  const step = niceStep(reach) / 5;
  const bands = Math.min(SPREAD_BANDS, Math.max(1, Math.ceil(reach / step)));
  const top = bands * step;
  const counts = new Array<number>(bands).fill(0);
  for (const distance of near) counts[Math.min(bands - 1, Math.floor(distance / step))]++;
  const shares = counts.map((count) => count / Math.max(1, near.length));
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
    yTicks: ticks(highest, y, (share) => formatPercent(share)),
    edge:
      usualRadius == null || usualRadius > top
        ? null
        : { at: x(usualRadius), label: `its edge ${formatDegrees(usualRadius)}` },
    median: mid == null ? null : { at: x(mid), label: `median ${formatDegrees(mid)}` },
  };
}

/**
 * The around map's grid: how far it reaches to each side (degrees), its scale (pixels a degree),
 * and its cells.
 */
interface AroundGrid {
  /** How far the map reaches up and down from the bot's center, in degrees. */
  reach: number;
  /** The chart's units a degree. */
  scale: number;
  /** How far the plot reaches behind and ahead of the bot's center, in degrees. */
  halfWidth: number;
  /** A cell's side, in degrees. */
  cell: number;
  /** How many whole cells fit across the plot. */
  columns: number;
}

/**
 * The map's grid: it reaches nearly every frame to the side (the 98th percentile), and past the
 * bot's edge; its cells are square. `radius` is the bot's usual radius, in degrees.
 */
function aroundGrid(points: AroundPoint[], radius: number, box: ChartBox): AroundGrid {
  const across = points.map((point) => Math.abs(point[1])).sort((a, b) => a - b);
  const reach = Math.max(1.5 * radius, across[Math.floor(0.98 * (across.length - 1))]);
  const scale = plotHeight(box) / 2 / reach;
  const halfWidth = plotWidth(box) / 2 / scale;
  const cell = (2 * reach) / MAP_ROWS;
  const columns = Math.floor((2 * halfWidth) / cell);
  return { reach, scale, halfWidth, cell, columns };
}

/** How many frames fall in each cell, by the cell's key (row * columns + column). */
function cellCounts(points: AroundPoint[], grid: AroundGrid): Map<number, number> {
  const { reach, cell, columns } = grid;
  const counts = new Map<number, number>();
  for (const [along, side] of points) {
    const column = Math.floor((along + (columns * cell) / 2) / cell);
    const row = Math.floor((side + reach) / cell);
    if (column < 0 || column >= columns || row < 0 || row >= MAP_ROWS) continue;
    const key = row * columns + column;
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return counts;
}

/**
 * The cells the crosshair sat in, shaded by how often (the square root, so rare places still show).
 */
function mapCells(
  points: AroundPoint[],
  grid: AroundGrid,
  center: ChartPoint,
  box: ChartBox,
): MapCell[] {
  const { reach, scale, cell, columns } = grid;
  const counts = cellCounts(points, grid);
  const most = Math.max(1, ...counts.values());
  const left = center.x - (columns * cell * scale) / 2;
  return [...counts].map(([key, count]): MapCell => {
    const row = Math.floor(key / columns);
    const column = key % columns;
    const along = (column + 0.5) * cell - (columns * cell) / 2;
    const side = (row + 0.5) * cell - reach;
    const way = along >= 0 ? 'ahead' : 'behind';
    const share = formatPercent(count / points.length);
    return {
      x: left + column * cell * scale,
      y: box.top + (MAP_ROWS - 1 - row) * cell * scale,
      size: cell * scale,
      shade: Math.sqrt(count / most),
      title: `${formatDegrees(Math.abs(along))} ${way}, ${formatDegrees(Math.abs(side))} to the side: ${share} of the time`,
    };
  });
}

/** The degrees along the bot's motion: behind it (minus) and ahead of it (plus), in round steps. */
function aroundTicks(grid: AroundGrid, center: ChartPoint): AxisTick[] {
  const { halfWidth, scale } = grid;
  const step = niceStep(halfWidth);
  const xTicks: AxisTick[] = [];
  for (
    let offset = -Math.floor(halfWidth / step) * step;
    offset <= halfWidth + ROUNDING_SLACK;
    offset += step
  ) {
    const label =
      Math.abs(offset) < ROUNDING_SLACK
        ? '0'
        : `${offset > 0 ? '+' : '−'}${formatDegrees(Math.abs(offset), step < 1 ? 1 : 0)}`;
    xTicks.push({ at: center.x + offset * scale, label });
  }
  return xTicks;
}

/**
 * Where the crosshair sat around the moving bot (the motion's frames, review.track_motion's
 * "around"); with no map and the reason when the motion was not measured or the review is older.
 */
export function aroundMap(report: TrackReport): AroundModel {
  const box = BOX;
  const motion = report.summary.motion;
  const points = motion?.around ?? [];
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
  if (!points.length) return empty(OLD_REVIEW);
  const radius = median(points.map((point) => point[2])) ?? 0;
  const grid = aroundGrid(points, radius, box);
  const usual = median(points.map((point) => point[0]));
  return {
    box,
    cells: mapCells(points, grid, center, box),
    center,
    radius: radius * grid.scale,
    usual: usual == null ? null : { x: center.x + usual * grid.scale, y: center.y },
    xTicks: aroundTicks(grid, center),
    reason: null,
  };
}

/**
 * How long the crosshair took to get back on the bot after each of its direction changes (the
 * motion's turns_back), at its time in the run: 0 when it stayed on, at the top when it was not
 * back before the next turn or the run's end.
 */
export function turnsBack(report: TrackReport, run: Timeline): TurnsBackModel {
  const box = BOX;
  const motion = report.summary.motion;
  const turns = motion?.turns_back;
  const empty = (reason: string): TurnsBackModel => ({
    box,
    dots: [],
    xTicks: [],
    yTicks: [],
    median: null,
    note: '',
    reason,
  });
  if (motion?.reason) return empty(`Not measured: ${motion.reason}.`);
  if (!turns) return empty(OLD_REVIEW_CHART);
  if (!turns.length) return empty('The bot did not change direction while you tracked it.');
  const backs = turns.flatMap((turn) => (turn.back == null ? [] : [turn.back]));
  const sorted = [...backs].sort((a, b) => a - b);
  const top = Math.max(
    TURNS_TOP_MIN,
    sorted.length ? sorted[Math.floor(0.98 * (sorted.length - 1))] : 0,
  );
  const runSeconds = run.frameCount / run.fps;
  const x = (seconds: number) =>
    box.left + (plotWidth(box) * seconds) / Math.max(WINDOW, runSeconds);
  const y = (seconds: number) => plotBottom(box) - (plotHeight(box) * Math.min(seconds, top)) / top;
  const dots = turns.map((turn): TurnDot => {
    const at = (turn.frame - run.start) / run.fps;
    const when = `Turn at ${Math.round(at)} s`;
    return {
      seconds: turn.frame / run.fps,
      x: x(at),
      y: turn.back == null ? box.top : y(turn.back),
      lost: turn.back == null,
      title:
        turn.back == null
          ? `${when}: not back on the bot before the next turn`
          : turn.back === 0
            ? `${when}: you stayed on the bot`
            : `${when}: back on the bot after ${formatSeconds(turn.back)}`,
    };
  });
  const mid = median(backs);
  const quick = backs.filter((back) => back <= QUICK_BACK + ROUNDING_SLACK).length / turns.length;
  const lost = turns.length - backs.length;
  const parts = [
    mid == null ? null : `Back on the bot a median ${formatSeconds(mid)} after a turn.`,
    `Back within ${formatMs(QUICK_BACK)} after ${formatPercent(quick)} of the ${turns.length} turns.`,
    lost ? `${lost} not back before the next turn.` : null,
  ];
  return {
    box,
    dots,
    xTicks: ticks(Math.max(WINDOW, runSeconds), x, (seconds) => `${Math.round(seconds)} s`),
    yTicks: ticks(top, y, (seconds) => formatMs(seconds)),
    median: mid == null ? null : { at: y(mid), label: `median ${formatSeconds(mid)}` },
    note: parts.filter((part) => part != null).join(' '),
    reason: null,
  };
}
