import { ClickReport, Flick } from '../../api';
import { formatMs } from '../../format';
import { PARTS } from '../report/budget';
import { fitFitts } from '../fastest-path/order-solver';
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
): AxisTick[] {
  const step = niceStep(top);
  const out: AxisTick[] = [];
  for (let v = 0; v <= top + 1e-9; v += step) out.push({ at: at(v), label: label(v) });
  return out;
}

const msLabel = (seconds: number) => String(Math.round(1000 * seconds));

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
  const every = flicks.length > 60 ? 20 : flicks.length > 20 ? 10 : 5;
  const xTicks = bars
    .filter((b) => b.flick.n === 1 || b.flick.n % every === 0)
    .map((b) => ({ at: b.x + b.width / 2, label: String(b.flick.n) }));
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
