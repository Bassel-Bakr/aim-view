/**
 * The flick speed profile chart's shapes, worked out from the report.
 *
 * In: the clicking summary's flick profile (the core's `FlickProfile`) and a kill's speed curve.
 * Out: SVG paths, points and ticks in the chart's viewBox units, which flick-profile.html draws.
 */

import { Flick, FlickProfile } from '../../api';
import { formatPercent } from '../../format';
import { ROUNDING_SLACK } from '../run-charts/run-charts-model';

/**
 * The chart's drawing box, in its own units (the SVG's viewBox), and the plot inside its margins.
 */
export interface ProfileBox {
  /** The viewBox's width. */
  width: number;
  /** The viewBox's height. */
  height: number;
  /** The margin left of the plot, where the speed labels go. */
  left: number;
  /** The margin right of the plot. */
  right: number;
  /** The margin above the plot. */
  top: number;
  /** The margin below the plot, where the time labels go. */
  bottom: number;
}

/** A labelled place on an axis, in the chart's units. */
export interface ProfileTick {
  /** The place along the axis, in the chart's units. */
  at: number;
  /** The tick's text, a percent. */
  label: string;
}

/** A point of the drawing, in the chart's units. */
export interface ProfilePoint {
  /** Across, from the viewBox's left. */
  x: number;
  /** Down, from the viewBox's top. */
  y: number;
}

/**
 * The flick speed profile as drawn: the average curve and the band of the middle half of the
 * flicks, the top speed's place on the average, the flick's end (100%), the axes, and the line
 * under the chart.
 */
export interface ProfileModel {
  /** The box the rest is drawn in. */
  box: ProfileBox;
  /** The average curve, as an SVG path. */
  mean: string;
  /** The band between the 25th and 75th percentiles, as a closed SVG path. */
  band: string;
  /** Where the top speed comes on the average curve. */
  peak: ProfilePoint;
  /** Where the flick ends (100% of it), across. */
  end: number;
  /** The time axis's ticks, every 25% of the flick. */
  xTicks: ProfileTick[];
  /** The speed axis's ticks, 0% to 100% of the top speed. */
  yTicks: ProfileTick[];
  /** The line under the chart: when the top speed comes and how long the braking takes. */
  summary: string;
  /** The share of a flick at the plot's right edge. */
  span: number;
}

/** The profile chart's box: a 520 by 200 viewBox with room for the labels. */
export const PROFILE_BOX: ProfileBox = {
  width: 520,
  height: 200,
  left: 44,
  right: 12,
  top: 10,
  bottom: 24,
};

/** The plot's height in shares of the top speed, a little over 1 so the top is not on the edge. */
const Y_TOP = 1.1;

/**
 * Where a share of the flick goes across, with `span` (the share at the right edge) filling the
 * plot.
 */
const xOf = (box: ProfileBox, span: number, share: number): number =>
  box.left + ((box.width - box.left - box.right) * share) / span;
/** Where a share of the top speed goes down; values over `Y_TOP` sit on the plot's top. */
const yOf = (box: ProfileBox, share: number): number =>
  box.height - box.bottom - ((box.height - box.top - box.bottom) * Math.min(share, Y_TOP)) / Y_TOP;
/** The points as one SVG path of straight lines, to a tenth of a unit. */
const line = (points: ProfilePoint[]): string =>
  points.map((point, i) => `${i ? 'L' : 'M'}${point.x.toFixed(1)},${point.y.toFixed(1)}`).join('');

/** The profile's chart, or null when the report has none (or its average has no points). */
export function profileChart(
  profile: FlickProfile | null | undefined,
  box = PROFILE_BOX,
): ProfileModel | null {
  if (!profile?.mean.length) return null;
  const span = profile.step * (profile.mean.length - 1);
  const at = (values: number[]): ProfilePoint[] =>
    values.map((value, index) => ({ x: xOf(box, span, index * profile.step), y: yOf(box, value) }));
  const upper = at(profile.p75);
  const lower = at(profile.p25).reverse();
  // the average's value where the top speed comes, between its two nearest points
  const peakIndex = Math.min(profile.mean.length - 2, Math.floor(profile.peak_at / profile.step));
  const fraction = profile.peak_at / profile.step - peakIndex;
  const peakValue =
    profile.mean[peakIndex] + (profile.mean[peakIndex + 1] - profile.mean[peakIndex]) * fraction;
  const xTicks: ProfileTick[] = [];
  for (let share = 0; share <= span + 1e-9; share += 0.25)
    xTicks.push({ at: xOf(box, span, share), label: formatPercent(share) });
  return {
    box,
    mean: line(at(profile.mean)),
    band: `${line([...upper, ...lower])}Z`,
    peak: { x: xOf(box, span, profile.peak_at), y: yOf(box, peakValue) },
    end: xOf(box, span, 1),
    xTicks,
    yTicks: [0, 0.25, 0.5, 0.75, 1].map((share) => ({
      at: yOf(box, share),
      label: formatPercent(share),
    })),
    summary:
      `Flick speed tops out at ${formatPercent(profile.peak_at)} of the flick; ` +
      `braking takes ${formatPercent(profile.braking)}.`,
    span,
  };
}

/**
 * One kill's own curve on the chart, as an SVG path: its camera speed as a share of its top speed
 * in the main flick, against its time as a share of the flick, up to the plot's right edge. Null
 * when no kill is given or it has no speed curve, or a curve too short to draw.
 */
export function killCurve(flick: Flick | null, chart: ProfileModel): string | null {
  const curve = flick?.speed;
  if (!curve || curve.flick_end < 1 || curve.speeds.length <= curve.flick_end) return null;
  const top = Math.max(...curve.speeds.slice(0, curve.flick_end + 1));
  if (top <= 0) return null;
  const points = curve.speeds
    .map((speed, i) => ({ share: i / curve.flick_end, speed: speed / top }))
    .filter((sample) => sample.share <= chart.span + ROUNDING_SLACK)
    .map((sample) => ({
      x: xOf(chart.box, chart.span, sample.share),
      y: yOf(chart.box, sample.speed),
    }));
  return line(points);
}
