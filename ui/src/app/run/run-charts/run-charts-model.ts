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
/** A float's rounding error is below this, so a value this close to a whole step counts as on it. */
export const ROUNDING_SLACK = 1e-9;
/** An axis reaches this far past the largest value it shows. */
const HEADROOM = 1.05;
/** The time axes reach at least this far (seconds; a flick's own axis less), the distance axes this far (degrees). */
const MIN_TOP_SECONDS = 0.2;
const MIN_FLICK_SECONDS = 0.1;
const MIN_FAR_DEG = 5;
/** A bar fills this share of its slot. */
const BAR_FILL = 0.7;
/** The speeds chart's span around the end of the main flick, in seconds. */
const SPEEDS_BEFORE = 0.3;
const SPEEDS_AFTER = 0.4;

export const plotWidth = (b: ChartBox) => b.width - b.left - b.right;
export const plotBottom = (b: ChartBox) => b.height - b.bottom;
export const plotHeight = (b: ChartBox) => plotBottom(b) - b.top;

/** A round step for an axis that reaches top, giving about four to six lines. */
export function niceStep(top: number): number {
  const raw = top / 5;
  const power = 10 ** Math.floor(Math.log10(raw));
  const unit = raw / power;
  return (unit < 1.5 ? 1 : unit < 3.5 ? 2 : unit < 7.5 ? 5 : 10) * power;
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
  for (let value = 0; value <= top + ROUNDING_SLACK; value += step)
    out.push({ at: at(value), label: label(value) });
  return out;
}

const msLabel = (seconds: number) => String(Math.round(1000 * seconds));

/** The kill numbers under a row of kills: the first kill and every 5th, 10th or 20th, by how many there are. */
function killTicks(kills: KillMark[]): AxisTick[] {
  const every = kills.length > 60 ? 20 : kills.length > 20 ? 10 : 5;
  return kills
    .filter((kill) => kill.flick.kill_number === 1 || kill.flick.kill_number % every === 0)
    .map((kill) => ({ at: kill.x, label: String(kill.flick.kill_number) }));
}

/** The kills' times, each as a bar of its five parts (a kill whose parts were not found: one grey bar). */
export function killTimes(report: ClickReport): KillTimesModel {
  const box = BOX;
  const flicks = report.flicks;
  const top = Math.max(MIN_TOP_SECONDS, ...flicks.map((kill) => kill.total)) * HEADROOM;
  const y = (seconds: number) => plotBottom(box) - (plotHeight(box) * seconds) / top;
  const slot = plotWidth(box) / Math.max(1, flicks.length);
  const bars = flicks.map((kill, i): KillBar => {
    const parts = kill.parts ?? null;
    const x = box.left + i * slot + (slot * (1 - BAR_FILL)) / 2;
    const segments: BarSegment[] = [];
    let at = 0;
    for (const [j, seconds] of (parts ?? [kill.total]).entries()) {
      segments.push({
        y: y(at + seconds),
        height: y(at) - y(at + seconds),
        color: parts ? PARTS[j].color : 'var(--axis)',
      });
      at += seconds;
    }
    const steps = parts
      ? ` (${parts.map((seconds, j) => `${PARTS[j].label.toLowerCase()} ${formatMs(seconds)}`).join(', ')})`
      : '';
    return {
      flick: kill,
      x,
      width: Math.max(1, slot * BAR_FILL),
      top: y(kill.total),
      segments,
      title: `Kill ${kill.kill_number}: ${formatMs(kill.total)}${steps}`,
    };
  });
  const xTicks = killTicks(bars.map((b) => ({ flick: b.flick, x: b.x + b.width / 2 })));
  const mid = median(flicks.map((kill) => kill.total));
  return {
    box,
    bars,
    xTicks,
    yTicks: ticks(top, y, msLabel),
    median: mid == null ? null : { at: y(mid), label: `median ${formatMs(mid)}` },
  };
}

/** Each kill's time against its distance, and the run's own Fitts' law curve. */
export function fittsChart(report: ClickReport): FittsModel {
  const box = BOX;
  const flicks = report.flicks;
  const fit = fitFitts(flicks, report.summary.radius);
  const far = Math.max(MIN_FAR_DEG, ...flicks.map((kill) => kill.D0)) * HEADROOM;
  const slow = Math.max(MIN_TOP_SECONDS, ...flicks.map((kill) => kill.total)) * HEADROOM;
  const x = (deg: number) => box.left + (plotWidth(box) * deg) / far;
  const y = (seconds: number) =>
    plotBottom(box) - (plotHeight(box) * Math.min(seconds, slow)) / slow;
  const predicted = (deg: number) => fit.a + fit.b * Math.log2(1 + deg / fit.widthDeg);
  const steps = 60;
  const curve = Array.from({ length: steps + 1 }, (_unused, i) => (far * i) / steps)
    .map(
      (deg, i) =>
        `${i ? 'L' : 'M'}${x(deg).toFixed(1)},${y(Math.max(0, predicted(deg))).toFixed(1)}`,
    )
    .join('');
  return {
    box,
    dots: flicks.map((kill) => ({
      flick: kill,
      x: x(kill.D0),
      y: y(kill.total),
      title: `Kill ${kill.kill_number}: ${kill.D0.toFixed(1)}°, ${formatMs(kill.total)} (the curve: ${formatMs(predicted(kill.D0))})`,
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
export function clickGroup(report: ClickReport): ClickGroupModel {
  const box = BOX;
  const radius = report.summary.radius;
  const placed = report.flicks
    .filter((kill) => kill.click_off_xy)
    .map((kill) => {
      const directionRad = (kill.direction_deg * Math.PI) / 180;
      const alongX = Math.cos(directionRad);
      const alongY = Math.sin(directionRad);
      // the crosshair from the target's center is minus the target's place from the crosshair
      const crosshairX = -kill.click_off_xy[0];
      const crosshairY = -kill.click_off_xy[1];
      return {
        flick: kill,
        along: crosshairX * alongX + crosshairY * alongY,
        across: -crosshairX * alongY + crosshairY * alongX,
      };
    });
  const distances = placed.map((spot) => Math.hypot(spot.along, spot.across)).sort((a, b) => a - b);
  const reach = Math.max(
    1.6 * radius,
    distances.length ? distances[Math.floor(0.95 * (distances.length - 1))] * 1.1 : 0,
  );
  const scale = plotHeight(box) / 2 / reach;
  const center: ChartPoint = { x: box.left + plotWidth(box) / 2, y: box.top + plotHeight(box) / 2 };
  const at = (along: number, across: number): ChartPoint => ({
    x: center.x + along * scale,
    y: center.y - across * scale,
  });
  const dots = placed.map((spot): ClickDot => {
    const point = at(spot.along, spot.across);
    const way = spot.along >= 0 ? 'past' : 'short of';
    return {
      flick: spot.flick,
      x: point.x,
      y: point.y,
      missed: (spot.flick.shots ?? 0) > 1,
      title: `Kill ${spot.flick.kill_number}: ${Math.abs(spot.along).toFixed(2)}° ${way} the center, ${Math.abs(spot.across).toFixed(2)}° to the side`,
    };
  });
  const count = placed.length;
  const average = count
    ? at(
        placed.reduce((sum, spot) => sum + spot.along, 0) / count,
        placed.reduce((sum, spot) => sum + spot.across, 0) / count,
      )
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
export function flickSpeeds(report: ClickReport): SpeedsModel {
  const box = BOX;
  const lined = report.flicks
    .filter(
      (kill): kill is ReactedFlick =>
        kill.react != null &&
        kill.flick != null &&
        !!report.paths[String(kill.kill_number)]?.length,
    )
    .map((kill) => {
      const end = kill.start_frame / report.fps + kill.react + kill.flick;
      const points = speeds(report.paths[String(kill.kill_number)], report.fps, true)
        .slice(1)
        .map(([frame, speed]): TimedSpeed => [frame / report.fps - end, speed])
        .filter(([seconds]) => seconds >= -SPEEDS_BEFORE && seconds <= SPEEDS_AFTER);
      return { flick: kill, points };
    })
    .filter((line) => line.points.length > 1);
  const peaks = lined
    .map((line) => Math.max(...line.points.map(([, speed]) => speed)))
    .sort((a, b) => a - b);
  const top = Math.max(60, peaks.length ? peaks[Math.floor(0.95 * (peaks.length - 1))] : 0) * 1.1;
  const span = SPEEDS_BEFORE + SPEEDS_AFTER;
  const x = (seconds: number) => box.left + (plotWidth(box) * (seconds + SPEEDS_BEFORE)) / span;
  const y = (speed: number) => plotBottom(box) - (plotHeight(box) * speed) / top;
  const path = (points: TimedSpeed[]) =>
    points
      .map(
        ([seconds, speed], i) => `${i ? 'L' : 'M'}${x(seconds).toFixed(1)},${y(speed).toFixed(1)}`,
      )
      .join('');
  // the median curve: at each frame's time, the median of the curves that reach it (each at its nearest point)
  const step = 1 / report.fps;
  const mid: TimedSpeed[] = [];
  for (let seconds = -SPEEDS_BEFORE; seconds <= SPEEDS_AFTER + ROUNDING_SLACK; seconds += step) {
    const speedsThen = lined
      .map((line) => line.points.find(([time]) => Math.abs(time - seconds) <= step / 2)?.[1])
      .filter((speed): speed is number => speed != null);
    const medianSpeed = speedsThen.length * 2 >= lined.length ? median(speedsThen) : null;
    if (medianSpeed != null) mid.push([seconds, medianSpeed]);
  }
  const xTicks: AxisTick[] = [];
  for (let ms = -200; ms <= 400; ms += 100)
    xTicks.push({ at: x(ms / 1000), label: `${ms > 0 ? '+' : ''}${ms}` });
  return {
    box,
    lines: lined.map((line) => ({ flick: line.flick, path: path(line.points) })),
    median: path(mid),
    zero: x(0),
    xTicks,
    yTicks: ticks(top, y, (speed) => String(Math.round(speed))),
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
/** Each of the eight directions spans this many degrees. */
const SECTOR_DEG = 45;
/** A distance group gives a median from this many flicks; a direction needs this many and this share of them. */
const MIN_GROUP_FLICKS = 3;
const MIN_DIRECTION_FLICKS = 3;
const MIN_DIRECTION_SHARE = 0.05;
/** The wheel's scale reaches at least this speed (25% above the run's median). */
const WHEEL_MIN_TOP = 1.25;
/** The pace chart's window, in seconds. */
const PACE_WINDOW = 10;

const percentLabel = (share: number) => `${Math.round(100 * share)}%`;
const signed = (value: number, digits: number) =>
  `${value > 0 ? '+' : ''}${(Math.abs(value) < ROUNDING_SLACK ? 0 : value).toFixed(digits)}`;
/** A flick with its main flick's time (null without a main flick). */
interface TimedFlick extends Flick {
  flick: number;
}

/** A flick with its reaction's and its main flick's time. */
interface ReactedFlick extends TimedFlick {
  react: number;
}

const timedFlick = (kill: Flick): kill is TimedFlick => kill.flick != null && kill.flick > 0;

/** Each kill's time as shares of its four parts (a kill whose parts were not found leaves a gap). */
export function killShares(report: ClickReport): KillSharesModel {
  const box = BOX;
  const flicks = report.flicks;
  const y = (share: number) => plotBottom(box) - plotHeight(box) * share;
  const slot = plotWidth(box) / Math.max(1, flicks.length);
  const width = Math.max(1, slot * BAR_FILL);
  const sums = KILL_STEPS.map(() => 0);
  const bars: KillBar[] = [];
  for (const [i, kill] of flicks.entries()) {
    const parts = kill.parts;
    if (!parts) continue;
    const times = KILL_STEPS.map((step) => step.parts.reduce((sum, j) => sum + parts[j], 0));
    const all = times.reduce((a, b) => a + b, 0);
    if (all <= 0) continue;
    let at = 0;
    const segments = times.map((seconds, j): BarSegment => {
      sums[j] += seconds;
      at += seconds;
      return {
        y: y(at / all),
        height: (plotHeight(box) * seconds) / all,
        color: KILL_STEPS[j].color,
      };
    });
    const said = times.map(
      (seconds, j) =>
        `${KILL_STEPS[j].label} ${formatPercent(seconds / all)} (${formatMs(seconds)})`,
    );
    bars.push({
      flick: kill,
      x: box.left + i * slot + (slot - width) / 2,
      width,
      top: box.top,
      segments,
      title: `Kill ${kill.kill_number}, ${CHART_WORDS.ttk} ${formatMs(kill.total)}: ${said.join(', ')}`,
    });
  }
  const whole = sums.reduce((a, b) => a + b, 0);
  return {
    box,
    bars,
    xTicks: killTicks(flicks.map((kill, i) => ({ flick: kill, x: box.left + (i + 0.5) * slot }))),
    yTicks: [0, 0.25, 0.5, 0.75, 1].map((share) => ({ at: y(share), label: percentLabel(share) })),
    legend: KILL_STEPS.map((step, j) => ({
      label: step.label,
      color: step.color,
      share: whole ? formatPercent(sums[j] / whole) : '–',
    })),
  };
}

/** Where a flick's main movement ended: short of the target's edge, past it, or on the target. */
function landingOf(kill: Flick, radius: number): Landing {
  if (kill.end_left > radius) return 'under';
  return kill.end_left < -radius ? 'over' : 'on';
}

function landingWords(kill: Flick, landing: Landing): string {
  if (landing === 'under') {
    return `${CHART_WORDS.underflick}, ${kill.end_left.toFixed(2)}° short of the center`;
  }
  if (landing === 'over') {
    return `${CHART_WORDS.overflick}, ${(-kill.end_left).toFixed(2)}° past the center`;
  }
  return `${CHART_WORDS.onTarget}, ${signed(-kill.end_left, 2)}° from the center`;
}

/** The landing chart's degrees: round steps from edge to edge, signed, with as many decimals as the step needs. */
function landingTicks(edge: number, x: (deg: number) => number): AxisTick[] {
  const tick = niceStep(2 * edge);
  const digits = Math.max(0, -Math.floor(Math.log10(tick) + ROUNDING_SLACK));
  const xTicks: AxisTick[] = [];
  const first = Math.ceil(-edge / tick - ROUNDING_SLACK);
  const last = Math.floor(edge / tick + ROUNDING_SLACK);
  for (let i = first; i <= last; i++) {
    xTicks.push({ at: x(i * tick), label: `${signed(i * tick, digits)}°` });
  }
  return xTicks;
}

/**
 * Where each flick's main movement ended, in degrees from the target's center along the flick (end_left turned
 * around: below 0 short of the center, above 0 past it), each flick a block in its column. Beyond the target's edge
 * it is an underflick or an overflick.
 */
export function landings(report: ClickReport): LandingModel {
  const box = BOX;
  const radius = report.summary.radius;
  const flicks = report.flicks.filter((kill) => Number.isFinite(kill.end_left));
  const along = (kill: Flick) => -kill.end_left;
  const far = flicks.map((kill) => Math.abs(along(kill))).sort((a, b) => a - b);
  const reach = Math.max(
    1,
    2 * radius,
    far.length ? far[Math.floor(LANDING_SHOWN * (far.length - 1))] : 0,
  );
  const step = niceStep(reach) / 2;
  const edge = Math.ceil(reach / step - ROUNDING_SLACK) * step;
  const columns = Math.round((2 * edge) / step);
  const x = (deg: number) =>
    box.left + (plotWidth(box) * (Math.max(-edge, Math.min(edge, deg)) + edge)) / (2 * edge);
  const counts = new Array<number>(columns).fill(0);
  const placed = flicks.map((kill) => {
    const column = Math.max(0, Math.min(columns - 1, Math.floor((along(kill) + edge) / step)));
    return { flick: kill, column, below: counts[column]++ };
  });
  const top = Math.max(4, ...counts);
  const y = (count: number) => plotBottom(box) - (plotHeight(box) * count) / top;
  const slot = plotWidth(box) / columns;
  const tall = plotHeight(box) / top;
  const gap = Math.min(1, tall / 4);
  const cells = placed.map(({ flick: kill, column, below }): LandingCell => {
    const landing = landingOf(kill, radius);
    return {
      flick: kill,
      x: box.left + column * slot + gap / 2,
      y: y(below + 1) + gap / 2,
      width: Math.max(1, slot - gap),
      height: Math.max(0.5, tall - gap),
      landing,
      title: `Kill ${kill.kill_number}: ${landingWords(kill, landing)}`,
    };
  });
  const mid = median(flicks.map(along));
  return {
    box,
    cells,
    xTicks: landingTicks(edge, x),
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
  const widthDeg = 2 * radius;
  const timed = flicks.filter((kill): kill is TimedFlick => timedFlick(kill) && kill.D0 > 0);
  const count = timed.length;
  if (count < 3 || widthDeg <= 0) return null;
  const xs = timed.map((kill) => Math.log2(1 + kill.D0 / widthDeg));
  const ys = timed.map((kill) => kill.flick);
  const mx = xs.reduce((a, b) => a + b, 0) / count;
  const my = ys.reduce((a, b) => a + b, 0) / count;
  const sxx = xs.reduce((sum, value) => sum + (value - mx) ** 2, 0);
  if (sxx <= 0) return null;
  const b = xs.reduce((sum, value, i) => sum + (value - mx) * (ys[i] - my), 0) / sxx;
  return { a: my - b * mx, b, widthDeg };
}

/** Each flick's time against its distance, with Fitts' law fitted to the run's flicks. */
export function flickTimes(report: ClickReport): FlickTimesModel {
  const box = BOX;
  const timed = report.flicks.filter((kill): kill is TimedFlick => timedFlick(kill) && kill.D0 > 0);
  const fit = fitFlickTimes(timed, report.summary.radius);
  const far = Math.max(MIN_FAR_DEG, ...timed.map((kill) => kill.D0)) * HEADROOM;
  const slow = Math.max(MIN_FLICK_SECONDS, ...timed.map((kill) => kill.flick)) * HEADROOM;
  const x = (deg: number) => box.left + (plotWidth(box) * deg) / far;
  const y = (seconds: number) =>
    plotBottom(box) - (plotHeight(box) * Math.max(0, Math.min(seconds, slow))) / slow;
  const predicted = (deg: number) => (fit ? fit.a + fit.b * Math.log2(1 + deg / fit.widthDeg) : 0);
  const steps = 60;
  const curve = fit
    ? Array.from({ length: steps + 1 }, (_unused, i) => (far * i) / steps)
        .map((deg, i) => `${i ? 'L' : 'M'}${x(deg).toFixed(1)},${y(predicted(deg)).toFixed(1)}`)
        .join('')
    : '';
  const word = CHART_WORDS.flick.toLowerCase();
  return {
    box,
    dots: timed.map((kill) => ({
      flick: kill,
      x: x(kill.D0),
      y: y(kill.flick),
      title: `Kill ${kill.kill_number}: ${kill.D0.toFixed(1)}°, ${word} ${formatMs(kill.flick)}${fit ? ` (the line: ${formatMs(predicted(kill.D0))})` : ''}`,
    })),
    curve,
    fit: fit
      ? `time = ${formatMs(fit.a)} + ${formatMs(fit.b)} × log2(1 + D / ${fit.widthDeg.toFixed(2)}°)`
      : null,
    xTicks: ticks(far, x, (deg) => `${Math.round(deg)}°`),
    yTicks: ticks(slow, y, msLabel),
  };
}

/** The direction (0 right, 90 up) as one of the eight: 0 for right, round to 7 for down-right. */
export function sector(directionDeg: number): number {
  return Math.round((((directionDeg % 360) + 360) % 360) / SECTOR_DEG) % DIRECTIONS.length;
}

/**
 * For each of the eight directions, each flick's speed toward it (the way it covered over its time) against the
 * median speed of its distance group. A group needs MIN_GROUP_FLICKS flicks to give a median.
 */
function relativeSpeeds(report: ClickReport): number[][] {
  const byDirection: number[][] = DIRECTIONS.map(() => []);
  for (const group of DISTANCE_GROUPS) {
    const inGroup = report.flicks
      .filter(
        (kill): kill is TimedFlick =>
          group.from <= kill.D0 &&
          kill.D0 < group.to &&
          timedFlick(kill) &&
          kill.D0 - kill.end_left > 0,
      )
      .map((kill) => ({ flick: kill, speed: (kill.D0 - kill.end_left) / kill.flick }));
    const groupMedian =
      inGroup.length >= MIN_GROUP_FLICKS ? median(inGroup.map((entry) => entry.speed)) : null;
    if (!groupMedian) continue;
    for (const entry of inGroup) {
      byDirection[sector(entry.flick.direction_deg)].push(entry.speed / groupMedian);
    }
  }
  return byDirection;
}

/** The direction whose speed wins by better(), among those with a speed; null when fewer than two have one. */
function pickDirection(
  speeds: (number | null)[],
  better: (a: number, b: number) => boolean,
): number | null {
  const known = speeds.flatMap((speed, direction) => (speed == null ? [] : [direction]));
  if (known.length < 2) return null;
  return known.reduce((chosen, direction) =>
    better(speeds[direction] ?? 0, speeds[chosen] ?? 0) ? direction : chosen,
  );
}

/** The wheel's center, the room its wedges reach to (the arrows sit past it), and the speed at that room's edge. */
interface WheelLayout {
  center: ChartPoint;
  outer: number;
  top: number;
}

/** A point on the wheel at an angle (degrees, 0 right, 90 up) and a radius, as an SVG path's "x,y". */
function wheelPoint(center: ChartPoint, angleDeg: number, radius: number): string {
  const angleRad = (angleDeg * Math.PI) / 180;
  const x = center.x + radius * Math.cos(angleRad);
  const y = center.y - radius * Math.sin(angleRad);
  return `${x.toFixed(1)},${y.toFixed(1)}`;
}

/** One direction's wedge: its speed's length, its arrow's place, and what it says. */
function wedge(
  name: Direction,
  direction: number,
  speed: number | null,
  flickCount: number,
  layout: WheelLayout,
  mark: WheelMark | null,
): DirectionWedge {
  const { center, outer, top } = layout;
  const radius = speed == null ? 0 : (outer * speed) / top;
  const middleDeg = direction * SECTOR_DEG;
  const middleRad = (middleDeg * Math.PI) / 180;
  const labelAt = outer + WHEEL_LABEL_ROOM / 2;
  const flicks = `${flickCount} ${flickCount === 1 ? 'flick' : 'flicks'}`;
  const from = wheelPoint(center, middleDeg - WEDGE_HALF, radius);
  const to = wheelPoint(center, middleDeg + WEDGE_HALF, radius);
  const arc = `A${radius.toFixed(1)},${radius.toFixed(1)} 0 0 0`;
  const speedWords = `${CHART_WORDS.flickSpeed.toLowerCase()} for their distance`;
  return {
    name,
    arrow: DIRECTION_ARROWS[name],
    path: speed == null ? '' : `M${center.x},${center.y}L${from}${arc} ${to}Z`,
    label: {
      x: center.x + labelAt * Math.cos(middleRad),
      y: center.y - labelAt * Math.sin(middleRad),
    },
    flicks: flickCount,
    speed,
    value: speed == null ? 'too few flicks' : `${speed.toFixed(2)}×`,
    mark,
    title:
      speed == null
        ? `${name}: ${flicks}, too few to compare`
        : `${name}: ${flicks}, ${speed.toFixed(2)} × the run's median ${speedWords}`,
  };
}

function markOf(direction: number, best: number | null, weakest: number | null): WheelMark | null {
  if (direction === best) return 'best';
  return direction === weakest ? 'weakest' : null;
}

/**
 * Flick speed toward each of the eight directions, as the what-if line "Flick every direction like your best one"
 * takes it: each flick's speed (the way it covered over its time) against the median of its distance group (groups
 * of 3 or more), and the median of those for each direction. A direction needs 3 flicks and 5% of them; the best
 * and weakest are marked when two or more have enough.
 */
export function directionWheel(report: ClickReport): DirectionWheelModel {
  const box = BOX;
  const byDirection = relativeSpeeds(report);
  const all = byDirection.reduce((sum, relative) => sum + relative.length, 0);
  const need = Math.max(MIN_DIRECTION_FLICKS, Math.ceil(MIN_DIRECTION_SHARE * all));
  const speeds = byDirection.map((relative) => (relative.length >= need ? median(relative) : null));
  const layout: WheelLayout = {
    center: { x: box.width / 2, y: box.height / 2 },
    outer: box.height / 2 - WHEEL_LABEL_ROOM,
    top: Math.max(WHEEL_MIN_TOP, ...speeds.map((speed) => speed ?? 0)) * HEADROOM,
  };
  const best = pickDirection(speeds, (a, b) => a > b);
  const weakest = pickDirection(speeds, (a, b) => a < b);
  const wedges = DIRECTIONS.map((name, direction) =>
    wedge(
      name,
      direction,
      speeds[direction],
      byDirection[direction].length,
      layout,
      markOf(direction, best, weakest),
    ),
  );
  return {
    box,
    center: layout.center,
    ring: layout.outer / layout.top,
    outer: layout.outer,
    wedges,
    best: best == null ? null : wedges[best],
    weakest: weakest == null ? null : wedges[weakest],
  };
}

/**
 * The kills in each 10 s of the run, as the what-if line "Keep up your best 10 seconds all run" counts them: windows
 * that start at the run's first flick or at a kill and end by the last kill, each counting the kills after its start
 * up to its end, drawn at the window's middle. The run's rate is its kills over the time from its first flick to
 * its last kill.
 */
export function pace(report: ClickReport): PaceModel {
  const box = BOX;
  const times = report.flicks.map((kill) => kill.kill_frame / report.fps).sort((a, b) => a - b);
  const first = report.flicks.length
    ? Math.min(...report.flicks.map((kill) => kill.start_frame)) / report.fps
    : 0;
  const last = times.at(-1) ?? first;
  const span = Math.max(PACE_WINDOW, last - first);
  const x = (seconds: number) => box.left + (plotWidth(box) * (seconds - first)) / span;
  const windows = [first, ...times]
    .filter((start) => start + PACE_WINDOW <= last)
    .map((start): PaceCount => ({
      start,
      kills: times.filter((time) => start < time && time <= start + PACE_WINDOW).length,
    }));
  const best = windows.reduce<PaceCount | null>(
    (bestSoFar, paceWindow) =>
      !bestSoFar || paceWindow.kills > bestSoFar.kills ? paceWindow : bestSoFar,
    null,
  );
  const counted = times.filter((time) => time > first).length;
  const rate = last - first >= PACE_WINDOW ? (PACE_WINDOW * counted) / (last - first) : null;
  const top = Math.max(4, best?.kills ?? 0, rate ?? 0) * 1.15;
  const y = (count: number) => plotBottom(box) - (plotHeight(box) * count) / top;
  const middle = (paceWindow: PaceCount) => x(paceWindow.start + PACE_WINDOW / 2);
  return {
    box,
    line: windows
      .map(
        (paceWindow, i) =>
          `${i ? 'L' : 'M'}${middle(paceWindow).toFixed(1)},${y(paceWindow.kills).toFixed(1)}`,
      )
      .join(''),
    best: best && {
      x: x(best.start),
      width: x(best.start + PACE_WINDOW) - x(best.start),
      peak: { x: middle(best), y: y(best.kills) },
      label: `best 10 s: ${best.kills} kills`,
    },
    average: rate == null ? null : { at: y(rate), label: `run ${rate.toFixed(1)}` },
    kills: report.flicks.map((kill) => ({ flick: kill, x: x(kill.kill_frame / report.fps) })),
    xTicks: ticks(
      span,
      (seconds) => x(first + seconds),
      (seconds) => `${Math.round(seconds)} s`,
    ),
    yTicks: ticks(top, y, String, 1),
  };
}
