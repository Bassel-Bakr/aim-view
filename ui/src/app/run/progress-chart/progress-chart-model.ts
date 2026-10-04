import { formatNumber } from '../../format';
import { PastRun } from '../../platform/score-history';

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const STAMP = /^(\d{4})\.(\d\d)\.(\d\d)-(\d\d)\.(\d\d)\.(\d\d)$/;
/** A stats file and a recording this many seconds apart or less are the same run. */
export const SAME_RUN_S = 5;
/** The runs the line takes the median of: this many, centered on each run. */
export const MEDIAN_RUNS = 9;

const MARGIN_LEFT = 48;
const MARGIN_RIGHT = 12;
const MARGIN_TOP = 10;
const MARGIN_BOTTOM = 22;
/** The room above and below the scores, as a share of their range. */
const SCORE_ROOM = 0.06;
/** Room for one date on the time axis, in pixels. */
const DATE_WIDTH = 110;

/** The chart's size in pixels. */
export interface HistorySize {
  width: number;
  height: number;
}

/** A run as a dot: where it is, the run, and its day's label. */
export interface HistoryDot {
  x: number;
  y: number;
  run: PastRun;
  /** When the run ended, in seconds (on the stamps' own clock). */
  seconds: number;
  date: string;
}

/** A run with its time as seconds. */
interface TimedRun {
  run: PastRun;
  seconds: number;
}

/** A point of a line. */
export interface HistoryPoint {
  x: number;
  y: number;
}

/** A horizontal grid line at a score, or a date on the time axis. */
export interface HistoryTick {
  at: number;
  label: string;
}

/**
 * Every run of a scenario over the days it was played (each day played gets the same width; its runs spread across
 * it in order), laid out in pixels: the dots, the median line, the personal best, and the run on the page.
 */
export interface HistoryModel {
  size: HistorySize;
  left: number;
  right: number;
  top: number;
  bottom: number;
  dots: HistoryDot[];
  median: HistoryPoint[];
  grid: HistoryTick[];
  dates: HistoryTick[];
  best: HistoryDot;
  /** This page's run among them, or null when none is within five seconds of its time. */
  current: HistoryDot | null;
  /** This run's place by score (1 is the best), or null. */
  rank: number | null;
  /** The line under the chart: runs, best, this run's place. */
  summary: string;
}

/** A file-name time stamp as seconds on one clock. The year 0026 reads as 2026. */
export function stampSeconds(stamp: string): number | null {
  const match = STAMP.exec(stamp);
  if (!match) return null;
  const year = Number(match[1].startsWith('00') ? `20${match[1].slice(2)}` : match[1]);
  const [, , month, day, hour, minute, second] = match.map(Number);
  return Date.UTC(year, month - 1, day, hour, minute, second) / 1000;
}

/** A day as "25 Apr 2024". */
function dateLabel(seconds: number): string {
  const date = new Date(seconds * 1000);
  return `${date.getUTCDate()} ${MONTHS[date.getUTCMonth()]} ${date.getUTCFullYear()}`;
}

/** The middle value (the mean of the two middle ones for an even count). */
function median(values: readonly number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length >> 1;
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

/** A round step for about four grid lines over a range. */
function gridStep(range: number): number {
  const rough = range / 4;
  const power = 10 ** Math.floor(Math.log10(rough));
  const unit = rough / power;
  return power * (unit < 1.5 ? 1 : unit < 3.5 ? 2 : unit < 7.5 ? 5 : 10);
}

/** "1st", "2nd", "3rd", "4th", "11th", "22nd". */
export function ordinal(place: number): string {
  const tens = place % 100;
  const suffix = tens >= 11 && tens <= 13 ? 'th' : (['th', 'st', 'nd', 'rd'][place % 10] ?? 'th');
  return `${place}${suffix}`;
}

/** The run among runs that ended within five seconds of a time stamp, the nearest; else -1. */
export function runAt(runs: readonly HistoryDot[], stamp: string): number {
  const seconds = stampSeconds(stamp);
  if (seconds === null) return -1;
  let found = -1;
  for (let i = 0; i < runs.length; i++) {
    const apart = Math.abs(runs[i].seconds - seconds);
    if (apart <= SAME_RUN_S && (found < 0 || apart < Math.abs(runs[found].seconds - seconds)))
      found = i;
  }
  return found;
}

/** Each day played as its label, and each run's day (an index into them): each day played is one slot. */
interface RunDays {
  labels: string[];
  dayOf: number[];
  runsPerDay: Map<number, number>;
}

function runDays(timed: TimedRun[]): RunDays {
  const labels: string[] = [];
  const dayOf = timed.map(({ seconds }) => {
    const label = dateLabel(seconds);
    if (labels.at(-1) !== label) labels.push(label);
    return labels.length - 1;
  });
  const runsPerDay = new Map<number, number>();
  for (const day of dayOf) runsPerDay.set(day, (runsPerDay.get(day) ?? 0) + 1);
  return { labels, dayOf, runsPerDay };
}

/** The score axis: its lowest and highest scores, the grid's step, and a score's height in pixels. */
interface ScoreAxis {
  floor: number;
  ceil: number;
  step: number;
  yOf: (score: number) => number;
}

/** The scores' range with a little room each side, rounded out to the grid's step. */
function scoreAxis(scores: number[], top: number, bottom: number): ScoreAxis {
  const [low, high] = [Math.min(...scores), Math.max(...scores)];
  const pad = (high - low) * SCORE_ROOM || Math.abs(high) * SCORE_ROOM || 1;
  const step = gridStep(high - low + 2 * pad);
  const [floor, ceil] = [
    Math.floor((low - pad) / step) * step,
    Math.ceil((high + pad) / step) * step,
  ];
  const yOf = (score: number) => bottom - ((bottom - top) * (score - floor)) / (ceil - floor);
  return { floor, ceil, step, yOf };
}

/** Each run's dot: its day's slot, shared in order by the day's runs, and its score's height. */
function historyDots(
  timed: TimedRun[],
  days: RunDays,
  left: number,
  slot: number,
  yOf: ScoreAxis['yOf'],
): HistoryDot[] {
  let inDay = 0;
  return timed.map(({ run, seconds }, i): HistoryDot => {
    inDay = i > 0 && days.dayOf[i] === days.dayOf[i - 1] ? inDay + 1 : 0;
    const runsThatDay = days.runsPerDay.get(days.dayOf[i]) ?? 1;
    return {
      x: left + slot * (days.dayOf[i] + (inDay + 0.5) / runsThatDay),
      y: yOf(run.score),
      run,
      seconds,
      date: days.labels[days.dayOf[i]],
    };
  });
}

/** The median line: at each run, the median of the runs around it. */
function medianLine(dots: HistoryDot[], scores: number[], yOf: ScoreAxis['yOf']): HistoryPoint[] {
  const half = MEDIAN_RUNS >> 1;
  return dots.map((dot, i) => ({
    x: dot.x,
    y: yOf(median(scores.slice(Math.max(0, i - half), i + half + 1))),
  }));
}

function gridTicks(axis: ScoreAxis): HistoryTick[] {
  const grid: HistoryTick[] = [];
  for (let score = axis.floor; score <= axis.ceil + axis.step / 2; score += axis.step)
    grid.push({ at: axis.yOf(score), label: formatNumber(score) });
  return grid;
}

/** The days' labels under their slots, leaving days out where they would not fit. */
function dateTicks(labels: string[], left: number, right: number, slot: number): HistoryTick[] {
  const every = Math.max(
    1,
    Math.ceil(labels.length / Math.max(1, Math.floor((right - left) / DATE_WIDTH))),
  );
  const dates: HistoryTick[] = [];
  for (let day = 0; day < labels.length; day += every)
    dates.push({ at: left + slot * (day + 0.5), label: labels[day] });
  return dates;
}

function summaryText(
  dots: HistoryDot[],
  best: HistoryDot,
  current: HistoryDot | null,
  rank: number | null,
): string {
  const parts = [
    `${formatNumber(dots.length)} ${dots.length === 1 ? 'run' : 'runs'} since ${dots[0].date}`,
    `best ${formatNumber(best.run.score)} on ${best.date}`,
    current && rank !== null
      ? `this run ${formatNumber(current.run.score)}: ${ordinal(rank)} best of ${formatNumber(dots.length)}`
      : 'this run is not among them',
  ];
  return parts.join(' · ');
}

/**
 * The chart of a scenario's runs (oldest first), with the run ended at thisStamp marked; null with no runs. The y
 * axis spans the scores with a little room; the line is the median of the runs around each one.
 */
export function progressChart(
  runs: readonly PastRun[],
  thisStamp: string | null,
  size: HistorySize,
): HistoryModel | null {
  const timed = runs
    .map((run) => ({ run, seconds: stampSeconds(run.stamp) }))
    .filter((timedRun): timedRun is TimedRun => timedRun.seconds !== null);
  if (!timed.length) return null;
  const [left, right, top, bottom] = [
    MARGIN_LEFT,
    size.width - MARGIN_RIGHT,
    MARGIN_TOP,
    size.height - MARGIN_BOTTOM,
  ];
  const days = runDays(timed);
  const slot = (right - left) / days.labels.length;
  const scores = timed.map((timedRun) => timedRun.run.score);
  const axis = scoreAxis(scores, top, bottom);
  const dots = historyDots(timed, days, left, slot, axis.yOf);
  const best = dots.reduce((a, b) => (b.run.score > a.run.score ? b : a));
  const at = thisStamp === null ? -1 : runAt(dots, thisStamp);
  const current = at < 0 ? null : dots[at];
  const rank = current && 1 + scores.filter((score) => score > current.run.score).length;
  return {
    size,
    left,
    right,
    top,
    bottom,
    dots,
    median: medianLine(dots, scores, axis.yOf),
    grid: gridTicks(axis),
    dates: dateTicks(days.labels, left, right, slot),
    best,
    current,
    rank,
    summary: summaryText(dots, best, current, rank),
  };
}

/** The dot nearest a point, within reach pixels; else null. */
export function dotNear(
  model: HistoryModel,
  x: number,
  y: number,
  reach: number,
): HistoryDot | null {
  let found: HistoryDot | null = null;
  let nearest = reach * reach;
  for (const dot of model.dots) {
    const distanceSquared = (dot.x - x) ** 2 + (dot.y - y) ** 2;
    if (distanceSquared <= nearest) {
      nearest = distanceSquared;
      found = dot;
    }
  }
  return found;
}
