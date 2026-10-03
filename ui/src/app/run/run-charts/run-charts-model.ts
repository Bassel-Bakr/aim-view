import { ClickReport, Direction, Flick } from '../../api';
import { DIRECTION_ARROWS, formatMs, formatPercent } from '../../format';
import { PARTS } from '../report/budget';
import { Fitts, fitFitts } from '../fastest-path/order-solver';
import { median } from '../median';
import { speeds } from '../speed';

/** A chart's drawing box, in its own units (the SVG's viewBox), and the plot inside its margins. */
export interface ChartBox {
  width: number;
  height: number;
  left: number;
  right: number;
  top: number;
  bottom: number;
}

/** A labelled place on an axis, in the chart's units. */
export interface AxisTick {
  at: number;
  label: string;
}

/** One part of a kill's bar. */
export interface BarSegment {
  y: number;
  height: number;
  color: string;
}

/** A kill's bar: its place, its parts from the bottom up, and what it says on hover. */
export interface KillBar {
  flick: Flick;
  x: number;
  width: number;
  top: number;
  segments: BarSegment[];
  title: string;
}

/** Every kill's time through the run, each in its five parts, and the run's median. */
export interface KillTimesModel {
  box: ChartBox;
  bars: KillBar[];
  xTicks: AxisTick[];
  yTicks: AxisTick[];
  /** The median kill's height, and its label. */
  median: AxisTick | null;
}

/** A kill as a dot. */
export interface ChartDot {
  flick: Flick;
  x: number;
  y: number;
  title: string;
}

/** Each kill's time against its distance, with the time Fitts' law fitted to the run gives for each distance. */
export interface FittsModel {
  box: ChartBox;
  dots: ChartDot[];
  curve: string;
  xTicks: AxisTick[];
  yTicks: AxisTick[];
}

/** A click on the target: a kill that took more than one shot is marked. */
export interface ClickDot extends ChartDot {
  missed: boolean;
}

/** A point in the chart's units. */
export interface ChartPoint {
  x: number;
  y: number;
}

/**
 * Where each click landed on its target, turned so every flick comes from the left: left of the center is short,
 * right is past. The target's edge, its center, and the clicks' average.
 */
export interface ClickGroupModel {
  box: ChartBox;
  center: ChartPoint;
  radius: number;
  dots: ClickDot[];
  average: ChartPoint | null;
  /** How many degrees the scale bar is, and its length. */
  scale: AxisTick;
}

/** A speed at a time: seconds from the end of the main flick, and degrees per second. */
export type TimedSpeed = [seconds: number, speed: number];

/** A flick's speed curve. */
export interface SpeedLine {
  flick: Flick;
  path: string;
}

/** Every flick's speed, lined up at the end of its main flick, and their median. */
export interface SpeedsModel {
  box: ChartBox;
  lines: SpeedLine[];
  median: string;
  /** Where the main flick ends: the time every curve is lined up at. */
  zero: number;
  xTicks: AxisTick[];
  yTicks: AxisTick[];
}

export const BOX: ChartBox = { width: 520, height: 200, left: 44, right: 12, top: 10, bottom: 24 };
/** A bar fills this share of its slot. */
const BAR_FILL = 0.7;
/** The speeds chart's span around the end of the main flick, in seconds. */
const SPEEDS_BEFORE = 0.3;
const SPEEDS_AFTER = 0.4;

const plotWidth = (b: ChartBox) => b.width - b.left - b.right;
const plotBottom = (b: ChartBox) => b.height - b.bottom;
const plotHeight = (b: ChartBox) => plotBottom(b) - b.top;

/** A round step for an axis that reaches top, giving about four to six lines. */
export function niceStep(top: number): number {
  const raw = top / 5;
  const p = 10 ** Math.floor(Math.log10(raw));
  const m = raw / p;
  return (m < 1.5 ? 1 : m < 3.5 ? 2 : m < 7.5 ? 5 : 10) * p;
}

/** The ticks from 0 to top, each placed by at() and labelled by label(). */
export function ticks(
  top: number,
  at: (v: number) => number,
  label: (v: number) => string,
  minStep = 0,
): AxisTick[] {
  const step = Math.max(minStep, niceStep(top));
  const out: AxisTick[] = [];
  for (let v = 0; v <= top + 1e-9; v += step) out.push({ at: at(v), label: label(v) });
  return out;
}

const msLabel = (seconds: number) => String(Math.round(1000 * seconds));

/** The kill numbers under a row of kills: the first kill and every 5th, 10th or 20th, by how many there are. */
function killTicks(kills: KillMark[]): AxisTick[] {
  const every = kills.length > 60 ? 20 : kills.length > 20 ? 10 : 5;
  return kills
    .filter((k) => k.flick.n === 1 || k.flick.n % every === 0)
    .map((k) => ({ at: k.x, label: String(k.flick.n) }));
}

/** The kills' times, each as a bar of its five parts (a kill whose parts were not found: one grey bar). */
export function killTimes(r: ClickReport): KillTimesModel {
  const box = BOX;
  const flicks = r.flicks;
  const top = Math.max(0.2, ...flicks.map((m) => m.total)) * 1.05;
  const y = (s: number) => plotBottom(box) - (plotHeight(box) * s) / top;
  const slot = plotWidth(box) / Math.max(1, flicks.length);
  const bars = flicks.map((m, i): KillBar => {
    const parts = m.parts ?? null;
    const x = box.left + i * slot + (slot * (1 - BAR_FILL)) / 2;
    const segments: BarSegment[] = [];
    let at = 0;
    for (const [j, seconds] of (parts ?? [m.total]).entries()) {
      segments.push({
        y: y(at + seconds),
        height: y(at) - y(at + seconds),
        color: parts ? PARTS[j].color : 'var(--axis)',
      });
      at += seconds;
    }
    const steps = parts
      ? ` (${parts.map((s, j) => `${PARTS[j].label.toLowerCase()} ${formatMs(s)}`).join(', ')})`
      : '';
    return {
      flick: m,
      x,
      width: Math.max(1, slot * BAR_FILL),
      top: y(m.total),
      segments,
      title: `Kill ${m.n}: ${formatMs(m.total)}${steps}`,
    };
  });
  const xTicks = killTicks(bars.map((b) => ({ flick: b.flick, x: b.x + b.width / 2 })));
  const mid = median(flicks.map((m) => m.total));
  return {
    box,
    bars,
    xTicks,
    yTicks: ticks(top, y, msLabel),
    median: mid == null ? null : { at: y(mid), label: `median ${formatMs(mid)}` },
  };
}

/** Each kill's time against its distance, and the run's own Fitts' law curve. */
export function fittsChart(r: ClickReport): FittsModel {
  const box = BOX;
  const flicks = r.flicks;
  const fit = fitFitts(flicks, r.summary.radius);
  const far = Math.max(5, ...flicks.map((m) => m.D0)) * 1.05;
  const slow = Math.max(0.2, ...flicks.map((m) => m.total)) * 1.05;
  const x = (deg: number) => box.left + (plotWidth(box) * deg) / far;
  const y = (s: number) => plotBottom(box) - (plotHeight(box) * Math.min(s, slow)) / slow;
  const predicted = (deg: number) => fit.a + fit.b * Math.log2(1 + deg / fit.W);
  const steps = 60;
  const curve = Array.from({ length: steps + 1 }, (_, i) => (far * i) / steps)
    .map(
      (deg, i) =>
        `${i ? 'L' : 'M'}${x(deg).toFixed(1)},${y(Math.max(0, predicted(deg))).toFixed(1)}`,
    )
    .join('');
  return {
    box,
    dots: flicks.map((m) => ({
      flick: m,
      x: x(m.D0),
      y: y(m.total),
      title: `Kill ${m.n}: ${m.D0.toFixed(1)}°, ${formatMs(m.total)} (the curve: ${formatMs(predicted(m.D0))})`,
    })),
    curve,
    xTicks: ticks(far, x, (deg) => `${Math.round(deg)}°`),
    yTicks: ticks(slow, y, msLabel),
  };
}

/**
 * The crosshair's place on the target at each click, turned so the flick comes from the left (along the flick to
 * the right, across it upward), and scaled so the target and nearly every click fit.
 */
export function clickGroup(r: ClickReport): ClickGroupModel {
  const box = BOX;
  const radius = r.summary.radius;
  const placed = r.flicks
    .filter((m) => m.click_off_xy)
    .map((m) => {
      const a = (m.dir * Math.PI) / 180;
      const ux = Math.cos(a);
      const uy = Math.sin(a);
      // the crosshair from the target's center is minus the target's place from the crosshair
      const px = -m.click_off_xy[0];
      const py = -m.click_off_xy[1];
      return { flick: m, along: px * ux + py * uy, across: -px * uy + py * ux };
    });
  const far = placed.map((p) => Math.hypot(p.along, p.across)).sort((a, b) => a - b);
  const reach = Math.max(
    1.6 * radius,
    far.length ? far[Math.floor(0.95 * (far.length - 1))] * 1.1 : 0,
  );
  const scale = plotHeight(box) / 2 / reach;
  const center: ChartPoint = { x: box.left + plotWidth(box) / 2, y: box.top + plotHeight(box) / 2 };
  const at = (along: number, across: number): ChartPoint => ({
    x: center.x + along * scale,
    y: center.y - across * scale,
  });
  const dots = placed.map((p): ClickDot => {
    const q = at(p.along, p.across);
    const way = p.along >= 0 ? 'past' : 'short of';
    return {
      flick: p.flick,
      x: q.x,
      y: q.y,
      missed: p.flick.shots > 1,
      title: `Kill ${p.flick.n}: ${Math.abs(p.along).toFixed(2)}° ${way} the center, ${Math.abs(p.across).toFixed(2)}° to the side`,
    };
  });
  const n = placed.length;
  const average = n
    ? at(placed.reduce((s, p) => s + p.along, 0) / n, placed.reduce((s, p) => s + p.across, 0) / n)
    : null;
  return {
    box,
    center,
    radius: radius * scale,
    dots,
    average,
    scale: { at: radius * scale, label: `${radius.toFixed(2)}°` },
  };
}

/** Each flick's speed (smoothed, as the speed chart's "Smooth" draws it), lined up at the end of its main flick. */
export function flickSpeeds(r: ClickReport): SpeedsModel {
  const box = BOX;
  const lined = r.flicks
    .filter((m) => m.react != null && m.flick != null && r.paths[String(m.n)]?.length)
    .map((m) => {
      const end = m.start_frame / r.fps + m.react + m.flick;
      const points = speeds(r.paths[String(m.n)], r.fps, true)
        .slice(1)
        .map(([f, v]): TimedSpeed => [f / r.fps - end, v])
        .filter(([t]) => t >= -SPEEDS_BEFORE && t <= SPEEDS_AFTER);
      return { flick: m, points };
    })
    .filter((l) => l.points.length > 1);
  const peaks = lined.map((l) => Math.max(...l.points.map(([, v]) => v))).sort((a, b) => a - b);
  const top = Math.max(60, peaks.length ? peaks[Math.floor(0.95 * (peaks.length - 1))] : 0) * 1.1;
  const span = SPEEDS_BEFORE + SPEEDS_AFTER;
  const x = (t: number) => box.left + (plotWidth(box) * (t + SPEEDS_BEFORE)) / span;
  const y = (v: number) => plotBottom(box) - (plotHeight(box) * v) / top;
  const path = (pts: TimedSpeed[]) =>
    pts.map(([t, v], i) => `${i ? 'L' : 'M'}${x(t).toFixed(1)},${y(v).toFixed(1)}`).join('');
  // the median curve: at each frame's time, the median of the curves that reach it (each at its nearest point)
  const step = 1 / r.fps;
  const mid: TimedSpeed[] = [];
  for (let t = -SPEEDS_BEFORE; t <= SPEEDS_AFTER + 1e-9; t += step) {
    const at = lined
      .map((l) => l.points.find(([u]) => Math.abs(u - t) <= step / 2)?.[1])
      .filter((v): v is number => v != null);
    const v = at.length * 2 >= lined.length ? median(at) : null;
    if (v != null) mid.push([t, v]);
  }
  const xTicks: AxisTick[] = [];
  for (let ms = -200; ms <= 400; ms += 100)
    xTicks.push({ at: x(ms / 1000), label: `${ms > 0 ? '+' : ''}${ms}` });
  return {
    box,
    lines: lined.map((l) => ({ flick: l.flick, path: path(l.points) })),
    median: path(mid),
    zero: x(0),
    xTicks,
    yTicks: ticks(top, y, (v) => String(Math.round(v))),
  };
}

/**
 * Every word the charts below use for a kill's parts and a flick's landing, in one place. Micro is the time budget's
 * "onto the target" and "settle" together.
 */
export const CHART_WORDS = {
  reaction: 'Reaction',
  flick: 'Flick',
  micro: 'Micro',
  confirmation: 'Confirmation',
  click: 'Click',
  underflick: 'Underflick',
  overflick: 'Overflick',
  onTarget: 'On the target',
  ttk: 'TTK',
  flickSpeed: 'Flick speed',
};

/** A part of a kill as the charts below draw it: its name, its color, and the budget's parts it adds up. */
export interface KillStep {
  label: string;
  color: string;
  /** Indexes into the kill's KillParts. */
  parts: number[];
}

/** A kill's four timed parts (the click ends the kill and takes no time). */
export const KILL_STEPS: KillStep[] = [
  { label: CHART_WORDS.reaction, color: PARTS[0].color, parts: [0] },
  { label: CHART_WORDS.flick, color: PARTS[1].color, parts: [1] },
  { label: CHART_WORDS.micro, color: PARTS[2].color, parts: [2, 3] },
  { label: CHART_WORDS.confirmation, color: PARTS[4].color, parts: [4] },
];

/** A part in a legend: its name and color, and its share of the run's time. */
export interface StepLegend {
  label: string;
  color: string;
  share: string;
}

/** Every kill's time as shares of its four parts, in the order of the run. */
export interface KillSharesModel {
  box: ChartBox;
  bars: KillBar[];
  xTicks: AxisTick[];
  yTicks: AxisTick[];
  legend: StepLegend[];
}

/** Where a flick's main movement ended: short of the target, on it, or past it. */
export type Landing = 'under' | 'on' | 'over';

/** A flick in the landing histogram: one block in its column. */
export interface LandingCell {
  flick: Flick;
  x: number;
  y: number;
  width: number;
  height: number;
  landing: Landing;
  title: string;
}

/** A stretch along an axis. */
export interface ChartSpan {
  from: number;
  to: number;
}

/** Where each flick ended against the target's center, every flick turned to come from the left. */
export interface LandingModel {
  box: ChartBox;
  cells: LandingCell[];
  xTicks: AxisTick[];
  yTicks: AxisTick[];
  zero: number;
  /** The target's edges: a flick that ends between them ends on the target. */
  target: ChartSpan;
  median: AxisTick | null;
}

/** Each flick's time against its distance, and Fitts' law fitted to the flicks. */
export interface FlickTimesModel {
  box: ChartBox;
  dots: ChartDot[];
  /** The fitted line; empty with too few flicks. */
  curve: string;
  /** The fit's numbers, in words; null with too few flicks. */
  fit: string | null;
  xTicks: AxisTick[];
  yTicks: AxisTick[];
}

/** The direction a wedge stands out for. */
export type WheelMark = 'best' | 'weakest';

/** One of the eight directions: its wedge (empty with too few flicks), its arrow's place, and its speed. */
export interface DirectionWedge {
  name: Direction;
  arrow: string;
  path: string;
  label: ChartPoint;
  flicks: number;
  /** The median flick speed toward it, against the run's median for the distance (1: the same). */
  speed: number | null;
  /** The speed in words: "1.12×". */
  value: string;
  mark: WheelMark | null;
  title: string;
}

/** Flick speed in each of the eight directions, as wedges around a center; the ring is the run's median speed. */
export interface DirectionWheelModel {
  box: ChartBox;
  center: ChartPoint;
  /** The ring's radius: a speed of 1, the run's median for the distance. */
  ring: number;
  outer: number;
  wedges: DirectionWedge[];
  best: DirectionWedge | null;
  weakest: DirectionWedge | null;
}

/** The best 10 s of the run: its stretch, the line's point there, and its label. */
export interface PaceWindow {
  x: number;
  width: number;
  peak: ChartPoint;
  label: string;
}

/** A kill's place on the run's time line. */
export interface KillMark {
  flick: Flick;
  x: number;
}

/** The kills in each 10 s of the run, its best 10 s, and the run's own rate. */
export interface PaceModel {
  box: ChartBox;
  line: string;
  best: PaceWindow | null;
  average: AxisTick | null;
  kills: KillMark[];
  xTicks: AxisTick[];
  yTicks: AxisTick[];
}

/** A 10 s window of the run: its start (seconds into the video) and the kills in it. */
interface PaceCount {
  start: number;
  kills: number;
}

/** The landing histogram's span takes in this share of the flicks; the rest go in its end columns. */
const LANDING_SHOWN = 0.98;
/** The direction wheel: each wedge's half width (degrees), and the room left for the arrows around it. */
const WEDGE_HALF = 20;
const WHEEL_LABEL_ROOM = 20;
/** The distance groups the direction wheel compares speeds within (degrees; src/summary.rs DISTANCES). */
const DISTANCE_GROUPS: ChartSpan[] = [
  { from: 0, to: 5 },
  { from: 5, to: 10 },
  { from: 10, to: 15 },
  { from: 15, to: 25 },
  { from: 25, to: 90 },
];
const DIRECTIONS = Object.keys(DIRECTION_ARROWS) as Direction[];
/** The pace chart's window, in seconds. */
const PACE_WINDOW = 10;

const percentLabel = (share: number) => `${Math.round(100 * share)}%`;
const signed = (v: number, digits: number) =>
  `${v > 0 ? '+' : ''}${(Math.abs(v) < 1e-9 ? 0 : v).toFixed(digits)}`;
const timedFlick = (m: Flick) => m.flick != null && m.flick > 0;

/** Each kill's time as shares of its four parts (a kill whose parts were not found leaves a gap). */
export function killShares(r: ClickReport): KillSharesModel {
  const box = BOX;
  const flicks = r.flicks;
  const y = (share: number) => plotBottom(box) - plotHeight(box) * share;
  const slot = plotWidth(box) / Math.max(1, flicks.length);
  const width = Math.max(1, slot * BAR_FILL);
  const sums = KILL_STEPS.map(() => 0);
  const bars: KillBar[] = [];
  for (const [i, m] of flicks.entries()) {
    const parts = m.parts;
    if (!parts) continue;
    const times = KILL_STEPS.map((s) => s.parts.reduce((t, j) => t + parts[j], 0));
    const all = times.reduce((a, b) => a + b, 0);
    if (all <= 0) continue;
    let at = 0;
    const segments = times.map((t, j): BarSegment => {
      sums[j] += t;
      at += t;
      return { y: y(at / all), height: (plotHeight(box) * t) / all, color: KILL_STEPS[j].color };
    });
    const said = times.map(
      (t, j) => `${KILL_STEPS[j].label} ${formatPercent(t / all)} (${formatMs(t)})`,
    );
    bars.push({
      flick: m,
      x: box.left + i * slot + (slot - width) / 2,
      width,
      top: box.top,
      segments,
      title: `Kill ${m.n}, ${CHART_WORDS.ttk} ${formatMs(m.total)}: ${said.join(', ')}`,
    });
  }
  const whole = sums.reduce((a, b) => a + b, 0);
  return {
    box,
    bars,
    xTicks: killTicks(flicks.map((m, i) => ({ flick: m, x: box.left + (i + 0.5) * slot }))),
    yTicks: [0, 0.25, 0.5, 0.75, 1].map((v) => ({ at: y(v), label: percentLabel(v) })),
    legend: KILL_STEPS.map((s, j) => ({
      label: s.label,
      color: s.color,
      share: whole ? formatPercent(sums[j] / whole) : '–',
    })),
  };
}

/**
 * Where each flick's main movement ended, in degrees from the target's center along the flick (end_left turned
 * around: below 0 short of the center, above 0 past it), each flick a block in its column. Beyond the target's edge
 * it is an underflick or an overflick.
 */
export function landings(r: ClickReport): LandingModel {
  const box = BOX;
  const radius = r.summary.radius;
  const flicks = r.flicks.filter((m) => Number.isFinite(m.end_left));
  const along = (m: Flick) => -m.end_left;
  const far = flicks.map((m) => Math.abs(along(m))).sort((a, b) => a - b);
  const reach = Math.max(
    1,
    2 * radius,
    far.length ? far[Math.floor(LANDING_SHOWN * (far.length - 1))] : 0,
  );
  const step = niceStep(reach) / 2;
  const edge = Math.ceil(reach / step - 1e-9) * step;
  const columns = Math.round((2 * edge) / step);
  const x = (deg: number) =>
    box.left + (plotWidth(box) * (Math.max(-edge, Math.min(edge, deg)) + edge)) / (2 * edge);
  const counts = new Array<number>(columns).fill(0);
  const placed = flicks.map((m) => {
    const column = Math.max(0, Math.min(columns - 1, Math.floor((along(m) + edge) / step)));
    return { flick: m, column, below: counts[column]++ };
  });
  const top = Math.max(4, ...counts);
  const y = (n: number) => plotBottom(box) - (plotHeight(box) * n) / top;
  const slot = plotWidth(box) / columns;
  const tall = plotHeight(box) / top;
  const gap = Math.min(1, tall / 4);
  const cells = placed.map(({ flick: m, column, below }): LandingCell => {
    const landing: Landing = m.end_left > radius ? 'under' : m.end_left < -radius ? 'over' : 'on';
    const how =
      landing === 'under'
        ? `${CHART_WORDS.underflick}, ${m.end_left.toFixed(2)}° short of the center`
        : landing === 'over'
          ? `${CHART_WORDS.overflick}, ${(-m.end_left).toFixed(2)}° past the center`
          : `${CHART_WORDS.onTarget}, ${signed(along(m), 2)}° from the center`;
    return {
      flick: m,
      x: box.left + column * slot + gap / 2,
      y: y(below + 1) + gap / 2,
      width: Math.max(1, slot - gap),
      height: Math.max(0.5, tall - gap),
      landing,
      title: `Kill ${m.n}: ${how}`,
    };
  });
  const tick = niceStep(2 * edge);
  const digits = Math.max(0, -Math.floor(Math.log10(tick) + 1e-9));
  const xTicks: AxisTick[] = [];
  for (let i = Math.ceil(-edge / tick - 1e-9); i <= Math.floor(edge / tick + 1e-9); i++)
    xTicks.push({ at: x(i * tick), label: `${signed(i * tick, digits)}°` });
  const mid = median(flicks.map(along));
  return {
    box,
    cells,
    xTicks,
    yTicks: ticks(top, y, String, 1),
    zero: x(0),
    target: { from: x(-radius), to: x(radius) },
    median: mid == null ? null : { at: x(mid), label: `median ${signed(mid, 2)}°` },
  };
}

/**
 * Fitts' law (time = a + b log2(1 + D / W), W the target's width) fitted by least squares to each flick's time
 * against its distance at the start; null with fewer than 3 flicks or a single distance.
 */
export function fitFlickTimes(flicks: Flick[], radius: number): Fitts | null {
  const W = 2 * radius;
  const timed = flicks.filter((m) => timedFlick(m) && m.D0 > 0);
  const n = timed.length;
  if (n < 3 || W <= 0) return null;
  const xs = timed.map((m) => Math.log2(1 + m.D0 / W));
  const ys = timed.map((m) => m.flick);
  const mx = xs.reduce((a, b) => a + b, 0) / n;
  const my = ys.reduce((a, b) => a + b, 0) / n;
  const sxx = xs.reduce((s, v) => s + (v - mx) ** 2, 0);
  if (sxx <= 0) return null;
  const b = xs.reduce((s, v, i) => s + (v - mx) * (ys[i] - my), 0) / sxx;
  return { a: my - b * mx, b, W };
}

/** Each flick's time against its distance, with Fitts' law fitted to the run's flicks. */
export function flickTimes(r: ClickReport): FlickTimesModel {
  const box = BOX;
  const timed = r.flicks.filter((m) => timedFlick(m) && m.D0 > 0);
  const fit = fitFlickTimes(timed, r.summary.radius);
  const far = Math.max(5, ...timed.map((m) => m.D0)) * 1.05;
  const slow = Math.max(0.1, ...timed.map((m) => m.flick)) * 1.05;
  const x = (deg: number) => box.left + (plotWidth(box) * deg) / far;
  const y = (s: number) =>
    plotBottom(box) - (plotHeight(box) * Math.max(0, Math.min(s, slow))) / slow;
  const predicted = (deg: number) => (fit ? fit.a + fit.b * Math.log2(1 + deg / fit.W) : 0);
  const steps = 60;
  const curve = fit
    ? Array.from({ length: steps + 1 }, (_, i) => (far * i) / steps)
        .map((deg, i) => `${i ? 'L' : 'M'}${x(deg).toFixed(1)},${y(predicted(deg)).toFixed(1)}`)
        .join('')
    : '';
  const word = CHART_WORDS.flick.toLowerCase();
  return {
    box,
    dots: timed.map((m) => ({
      flick: m,
      x: x(m.D0),
      y: y(m.flick),
      title: `Kill ${m.n}: ${m.D0.toFixed(1)}°, ${word} ${formatMs(m.flick)}${fit ? ` (the line: ${formatMs(predicted(m.D0))})` : ''}`,
    })),
    curve,
    fit: fit
      ? `time = ${formatMs(fit.a)} + ${formatMs(fit.b)} × log2(1 + D / ${fit.W.toFixed(2)}°)`
      : null,
    xTicks: ticks(far, x, (deg) => `${Math.round(deg)}°`),
    yTicks: ticks(slow, y, msLabel),
  };
}

/** The direction (0 right, 90 up) as one of the eight: 0 for right, round to 7 for down-right. */
export function sector(dir: number): number {
  return Math.round((((dir % 360) + 360) % 360) / 45) % 8;
}

/**
 * Flick speed toward each of the eight directions, as the what-if line "Flick every direction like your best one"
 * takes it: each flick's speed (the way it covered over its time) against the median of its distance group (groups
 * of 3 or more), and the median of those for each direction. A direction needs 3 flicks and 5% of them; the best
 * and weakest are marked when two or more have enough.
 */
export function directionWheel(r: ClickReport): DirectionWheelModel {
  const box = BOX;
  const groups: number[][] = DIRECTIONS.map(() => []);
  for (const g of DISTANCE_GROUPS) {
    const fast = r.flicks
      .filter((m) => g.from <= m.D0 && m.D0 < g.to && timedFlick(m) && m.D0 - m.end_left > 0)
      .map((m) => ({ flick: m, speed: (m.D0 - m.end_left) / m.flick }));
    const mid = fast.length >= 3 ? median(fast.map((p) => p.speed)) : null;
    if (!mid) continue;
    for (const p of fast) groups[sector(p.flick.dir)].push(p.speed / mid);
  }
  const all = groups.reduce((a, g) => a + g.length, 0);
  const need = Math.max(3, Math.ceil(0.05 * all));
  const speeds = groups.map((g) => (g.length >= need ? median(g) : null));
  const center: ChartPoint = { x: box.width / 2, y: box.height / 2 };
  const outer = box.height / 2 - WHEEL_LABEL_ROOM;
  const top = Math.max(1.25, ...speeds.map((v) => v ?? 0)) * 1.05;
  const at = (deg: number, radius: number): string => {
    const a = (deg * Math.PI) / 180;
    return `${(center.x + radius * Math.cos(a)).toFixed(1)},${(center.y - radius * Math.sin(a)).toFixed(1)}`;
  };
  const known = speeds.flatMap((v, k) => (v == null ? [] : [k]));
  const pick = (better: (a: number, b: number) => boolean) =>
    known.length < 2
      ? null
      : known.reduce((b, k) => (better(speeds[k] ?? 0, speeds[b] ?? 0) ? k : b));
  const bestK = pick((a, b) => a > b);
  const weakestK = pick((a, b) => a < b);
  const wedges = DIRECTIONS.map((name, k): DirectionWedge => {
    const v = speeds[k];
    const radius = v == null ? 0 : (outer * v) / top;
    const mid = k * 45;
    const a = (mid * Math.PI) / 180;
    const labelAt = outer + WHEEL_LABEL_ROOM / 2;
    const n = groups[k].length;
    const count = `${n} ${n === 1 ? 'flick' : 'flicks'}`;
    return {
      name,
      arrow: DIRECTION_ARROWS[name],
      path:
        v == null
          ? ''
          : `M${center.x},${center.y}L${at(mid - WEDGE_HALF, radius)}A${radius.toFixed(1)},${radius.toFixed(1)} 0 0 0 ${at(mid + WEDGE_HALF, radius)}Z`,
      label: { x: center.x + labelAt * Math.cos(a), y: center.y - labelAt * Math.sin(a) },
      flicks: n,
      speed: v,
      value: v == null ? 'too few flicks' : `${v.toFixed(2)}×`,
      mark: k === bestK ? 'best' : k === weakestK ? 'weakest' : null,
      title:
        v == null
          ? `${name}: ${count}, too few to compare`
          : `${name}: ${count}, ${v.toFixed(2)} × the run's median ${CHART_WORDS.flickSpeed.toLowerCase()} for their distance`,
    };
  });
  return {
    box,
    center,
    ring: outer / top,
    outer,
    wedges,
    best: bestK == null ? null : wedges[bestK],
    weakest: weakestK == null ? null : wedges[weakestK],
  };
}

/**
 * The kills in each 10 s of the run, as the what-if line "Keep up your best 10 seconds all run" counts them: windows
 * that start at the run's first flick or at a kill and end by the last kill, each counting the kills after its start
 * up to its end, drawn at the window's middle. The run's rate is its kills over the time from its first flick to
 * its last kill.
 */
export function pace(r: ClickReport): PaceModel {
  const box = BOX;
  const times = r.flicks.map((m) => m.kill_frame / r.fps).sort((a, b) => a - b);
  const first = r.flicks.length ? Math.min(...r.flicks.map((m) => m.start_frame)) / r.fps : 0;
  const last = times.at(-1) ?? first;
  const span = Math.max(PACE_WINDOW, last - first);
  const x = (t: number) => box.left + (plotWidth(box) * (t - first)) / span;
  const windows = [first, ...times]
    .filter((s) => s + PACE_WINDOW <= last)
    .map((s): PaceCount => ({
      start: s,
      kills: times.filter((k) => s < k && k <= s + PACE_WINDOW).length,
    }));
  const best = windows.reduce<PaceCount | null>((b, w) => (!b || w.kills > b.kills ? w : b), null);
  const counted = times.filter((t) => t > first).length;
  const rate = last - first >= PACE_WINDOW ? (PACE_WINDOW * counted) / (last - first) : null;
  const top = Math.max(4, best?.kills ?? 0, rate ?? 0) * 1.15;
  const y = (k: number) => plotBottom(box) - (plotHeight(box) * k) / top;
  const middle = (w: PaceCount) => x(w.start + PACE_WINDOW / 2);
  return {
    box,
    line: windows
      .map((w, i) => `${i ? 'L' : 'M'}${middle(w).toFixed(1)},${y(w.kills).toFixed(1)}`)
      .join(''),
    best: best && {
      x: x(best.start),
      width: x(best.start + PACE_WINDOW) - x(best.start),
      peak: { x: middle(best), y: y(best.kills) },
      label: `best 10 s: ${best.kills} kills`,
    },
    average: rate == null ? null : { at: y(rate), label: `run ${rate.toFixed(1)}` },
    kills: r.flicks.map((m) => ({ flick: m, x: x(m.kill_frame / r.fps) })),
    xTicks: ticks(
      span,
      (v) => x(first + v),
      (v) => `${Math.round(v)} s`,
    ),
    yTicks: ticks(top, y, String, 1),
  };
}
