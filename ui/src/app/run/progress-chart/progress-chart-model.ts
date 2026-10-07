/**
 * The progress chart's layout: every past run of a scenario, laid out in pixels.
 *
 * In: the scenario's past runs from KovaaK's stats files (platform/score-history.ts `PastRun`), the
 * time stamp of the run on the page, and the chart's size.
 * Out: the dots, median line, grid and dates (progress-chart-drawing.ts draws them), and the
 * helpers the chart uses to match a dot to a recording.
 */

import { formatNumber } from '../../format';
import { PastRun } from '../../platform/score-history';

/** The months' short names, for the dates. */
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
/** A file name's time stamp: "2026.10.07-14.03.59". */
const STAMP = /^(\d{4})\.(\d\d)\.(\d\d)-(\d\d)\.(\d\d)\.(\d\d)$/;
/** A stats file and a recording this many seconds apart or less are the same run. */
export const SAME_RUN_S = 5;
/** The runs the line takes the median of: this many, centered on each run. */
export const MEDIAN_RUNS = 9;

/** The plot's left margin, for the scores, in pixels. */
const MARGIN_LEFT = 48;
/** The plot's right margin, in pixels. */
const MARGIN_RIGHT = 12;
/** The plot's top margin, in pixels. */
const MARGIN_TOP = 10;
/** The plot's bottom margin, for the dates, in pixels. */
const MARGIN_BOTTOM = 22;
/** The room above and below the scores, as a share of their range. */
const SCORE_ROOM = 0.06;
/** Room for one date on the time axis, in pixels. */
const DATE_WIDTH = 110;

/** The chart's size in pixels. */
export interface HistorySize {
  /** The chart's width, in pixels. */
  width: number;
  /** The chart's height, in pixels. */
  height: number;
}

/** A run as a dot: where it is, the run, and its day's label. */
export interface HistoryDot {
  /** The dot's place across, in pixels. */
  x: number;
  /** The dot's height for its score, in pixels from the top. */
  y: number;
  /** The run: its time stamp, score, kills and accuracy. */
  run: PastRun;
  /** When the run ended, in seconds (on the stamps' own clock). */
  seconds: number;
  /** The run's day, "25 Apr 2024". */
  date: string;
}

/** A run with its time as seconds. */
interface TimedRun {
  /** The run. */
  run: PastRun;
  /** When it ended, in seconds on the stamps' clock. */
  seconds: number;
}

/** A point of a line. */
export interface HistoryPoint {
  /** Across, in pixels. */
  x: number;
  /** Down, in pixels from the top. */
  y: number;
}

/** A horizontal grid line at a score, or a date on the time axis. */
export interface HistoryTick {
  /** The grid line's height, or the date's place across, in pixels. */
  at: number;
  /** The score or the date. */
  label: string;
}

/**
 * Every run of a scenario over the days it was played (each day played gets the same width; its
 * runs spread across it in order), laid out in pixels: the dots, the median line, the personal
 * best, and the run on the page.
 */
export interface HistoryModel {
  /** The chart's size. */
  size: HistorySize;
  /** The plot's left edge, in pixels. */
  left: number;
  /** The plot's right edge, in pixels. */
  right: number;
  /** The plot's top, in pixels. */
  top: number;
  /** The plot's bottom, in pixels. */
  bottom: number;
  /** One dot for each run, oldest first. */
  dots: HistoryDot[];
  /** The median line, a point at each run. */
  median: HistoryPoint[];
  /** The score grid's lines. */
  grid: HistoryTick[];
  /** The days along the bottom, those that fit. */
  dates: HistoryTick[];
  /** The personal best's dot (the first, on a tie). */
  best: HistoryDot;
  /** This page's run among them, or null when none is within five seconds of its time. */
  current: HistoryDot | null;
  /** This run's place by score (1 is the best), or null. */
  rank: number | null;
  /** The line under the chart: runs, best, this run's place. */
  summary: string;
}

/**
 * A file-name time stamp as seconds on one clock (read as UTC, so the dates shown are the stamps'
 * own days in any time zone); null when it is not a stamp. The year 0026 reads as 2026.
 */
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

/**
 * Each day played as its label, and each run's day (an index into them): each day played is one
 * slot.
 */
interface RunDays {
  /** Each day played, in order, as "25 Apr 2024". */
  labels: string[];
  /** Each run's day, an index into `labels`. */
  dayOf: number[];
  /** How many runs each day has, by its index. */
  runsPerDay: Map<number, number>;
}

/** The days the runs (oldest first) were played on, and each run's day. */
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

/**
 * The score axis: its lowest and highest scores, the grid's step, and a score's height in pixels.
 */
interface ScoreAxis {
  /** The score at the plot's bottom, a whole number of steps. */
  floor: number;
  /** The score at the plot's top, a whole number of steps. */
  ceil: number;
  /** The scores between grid lines. */
  step: number;
  /** A score's height, in pixels from the top. */
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

/** A grid line at every step of the score axis, from its floor to its ceiling. */
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

/** The line under the chart: how many runs since when, the best, and this run's place. */
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
 * The chart of a scenario's runs (oldest first), with the run ended at thisStamp marked; null with
 * no runs (runs whose stamp cannot be read are left out). The y axis spans the scores with a little
 * room; the line is the median of the runs around each one.
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

/** The dot nearest a point (pixels), within `reach` pixels of it; else null. */
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
