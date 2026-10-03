import { formatNumber } from '../../format';
import { PastRun } from '../../platform/score-history';

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const STAMP = /^(\d{4})\.(\d\d)\.(\d\d)-(\d\d)\.(\d\d)\.(\d\d)$/;
/** A stats file and a recording this many seconds apart or less are the same run. */
const SAME_RUN_S = 5;
/** The runs the line takes the median of: this many, centered on each run. */
export const MEDIAN_RUNS = 9;

const MARGIN_LEFT = 48;
const MARGIN_RIGHT = 12;
const MARGIN_TOP = 10;
const MARGIN_BOTTOM = 22;
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
  const m = STAMP.exec(stamp);
  if (!m) return null;
  const year = Number(m[1].startsWith('00') ? `20${m[1].slice(2)}` : m[1]);
  const [, , mo, d, h, mi, s] = m.map(Number);
  return Date.UTC(year, mo - 1, d, h, mi, s) / 1000;
}

/** A day as "25 Apr 2024". */
function dateLabel(seconds: number): string {
  const d = new Date(seconds * 1000);
  return `${d.getUTCDate()} ${MONTHS[d.getUTCMonth()]} ${d.getUTCFullYear()}`;
}

/** The middle value (the mean of the two middle ones for an even count). */
function median(values: readonly number[]): number {
  const s = [...values].sort((a, b) => a - b);
  const mid = s.length >> 1;
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
}

/** A round step for about four grid lines over a range. */
function gridStep(range: number): number {
  const rough = range / 4;
  const power = 10 ** Math.floor(Math.log10(rough));
  const unit = rough / power;
  return power * (unit < 1.5 ? 1 : unit < 3.5 ? 2 : unit < 7.5 ? 5 : 10);
}

/** "1st", "2nd", "3rd", "4th", "11th", "22nd". */
export function ordinal(n: number): string {
  const tens = n % 100;
  const suffix = tens >= 11 && tens <= 13 ? 'th' : (['th', 'st', 'nd', 'rd'][n % 10] ?? 'th');
  return `${n}${suffix}`;
}

/** The run among runs that ended within five seconds of a time stamp, the nearest; else -1. */
export function runAt(runs: readonly HistoryDot[], stamp: string): number {
  const t = stampSeconds(stamp);
  if (t === null) return -1;
  let found = -1;
  for (let i = 0; i < runs.length; i++) {
    const d = Math.abs(runs[i].seconds - t);
    if (d <= SAME_RUN_S && (found < 0 || d < Math.abs(runs[found].seconds - t))) found = i;
  }
  return found;
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
    .filter((r): r is TimedRun => r.seconds !== null);
  if (!timed.length) return null;
  const [left, right, top, bottom] = [
    MARGIN_LEFT,
    size.width - MARGIN_RIGHT,
    MARGIN_TOP,
    size.height - MARGIN_BOTTOM,
  ];
  // each day played is one slot
  const days: string[] = [];
  const dayOf = timed.map(({ seconds }) => {
    const label = dateLabel(seconds);
    if (days.at(-1) !== label) days.push(label);
    return days.length - 1;
  });
  const perDay = new Map<number, number>();
  for (const d of dayOf) perDay.set(d, (perDay.get(d) ?? 0) + 1);
  const slot = (right - left) / days.length;
  const scores = timed.map((r) => r.run.score);
  const [low, high] = [Math.min(...scores), Math.max(...scores)];
  const pad = (high - low) * 0.06 || Math.abs(high) * 0.06 || 1;
  const step = gridStep(high - low + 2 * pad);
  const [floor, ceil] = [
    Math.floor((low - pad) / step) * step,
    Math.ceil((high + pad) / step) * step,
  ];
  const yOf = (v: number) => bottom - ((bottom - top) * (v - floor)) / (ceil - floor);
  let inDay = 0;
  const dots = timed.map(({ run, seconds }, i): HistoryDot => {
    inDay = i > 0 && dayOf[i] === dayOf[i - 1] ? inDay + 1 : 0;
    const n = perDay.get(dayOf[i]) ?? 1;
    return {
      x: left + slot * (dayOf[i] + (inDay + 0.5) / n),
      y: yOf(run.score),
      run,
      seconds,
      date: days[dayOf[i]],
    };
  });
  const half = MEDIAN_RUNS >> 1;
  const medianLine = dots.map((d, i) => ({
    x: d.x,
    y: yOf(median(scores.slice(Math.max(0, i - half), i + half + 1))),
  }));
  const grid: HistoryTick[] = [];
  for (let v = floor; v <= ceil + step / 2; v += step)
    grid.push({ at: yOf(v), label: formatNumber(v) });
  const every = Math.max(
    1,
    Math.ceil(days.length / Math.max(1, Math.floor((right - left) / DATE_WIDTH))),
  );
  const dates: HistoryTick[] = [];
  for (let d = 0; d < days.length; d += every)
    dates.push({ at: left + slot * (d + 0.5), label: days[d] });
  const best = dots.reduce((a, b) => (b.run.score > a.run.score ? b : a));
  const at = thisStamp === null ? -1 : runAt(dots, thisStamp);
  const current = at < 0 ? null : dots[at];
  const rank = current && 1 + scores.filter((s) => s > current.run.score).length;
  const parts = [
    `${formatNumber(dots.length)} ${dots.length === 1 ? 'run' : 'runs'} since ${dots[0].date}`,
    `best ${formatNumber(best.run.score)} on ${best.date}`,
    current && rank !== null
      ? `this run ${formatNumber(current.run.score)}: ${ordinal(rank)} best of ${formatNumber(dots.length)}`
      : 'this run is not among them',
  ];
  return {
    size,
    left,
    right,
    top,
    bottom,
    dots,
    median: medianLine,
    grid,
    dates,
    best,
    current,
    rank,
    summary: parts.join(' · '),
  };
}

/** The dot nearest a point, within reach pixels; else null. */
export function dotNear(m: HistoryModel, x: number, y: number, reach: number): HistoryDot | null {
  let found: HistoryDot | null = null;
  let nearest = reach * reach;
  for (const d of m.dots) {
    const dist = (d.x - x) ** 2 + (d.y - y) ** 2;
    if (dist <= nearest) {
      nearest = dist;
      found = d;
    }
  }
  return found;
}
