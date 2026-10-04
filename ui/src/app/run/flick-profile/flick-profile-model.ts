import { Flick, FlickProfile } from '../../api';
import { formatPercent } from '../../format';

/** The chart's drawing box, in its own units (the SVG's viewBox), and the plot inside its margins. */
export interface ProfileBox {
  width: number;
  height: number;
  left: number;
  right: number;
  top: number;
  bottom: number;
}

/** A labelled place on an axis, in the chart's units. */
export interface ProfileTick {
  at: number;
  label: string;
}

/** A point of the drawing, in the chart's units. */
export interface ProfilePoint {
  x: number;
  y: number;
}

/**
 * The flick speed profile as drawn: the average curve and the band of the middle half of the flicks, the top speed's
 * place on the average, the flick's end (100%), the axes, and the line under the chart.
 */
export interface ProfileModel {
  box: ProfileBox;
  mean: string;
  band: string;
  peak: ProfilePoint;
  end: number;
  xTicks: ProfileTick[];
  yTicks: ProfileTick[];
  summary: string;
  /** The share of a flick at the plot's right edge. */
  span: number;
}

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

const xOf = (box: ProfileBox, span: number, share: number): number =>
  box.left + ((box.width - box.left - box.right) * share) / span;
const yOf = (box: ProfileBox, v: number): number =>
  box.height - box.bottom - ((box.height - box.top - box.bottom) * Math.min(v, Y_TOP)) / Y_TOP;
const line = (points: ProfilePoint[]): string =>
  points.map((p, i) => `${i ? 'L' : 'M'}${p.x.toFixed(1)},${p.y.toFixed(1)}`).join('');

/** The profile's chart, or null when the report has none. */
export function profileChart(
  p: FlickProfile | null | undefined,
  box = PROFILE_BOX,
): ProfileModel | null {
  if (!p?.mean.length) return null;
  const span = p.step * (p.mean.length - 1);
  const at = (values: number[]): ProfilePoint[] =>
    values.map((v, k) => ({ x: xOf(box, span, k * p.step), y: yOf(box, v) }));
  const upper = at(p.p75);
  const lower = at(p.p25).reverse();
  // the average's value where the top speed comes, between its two nearest points
  const k = Math.min(p.mean.length - 2, Math.floor(p.peak_at / p.step));
  const t = p.peak_at / p.step - k;
  const peakValue = p.mean[k] + (p.mean[k + 1] - p.mean[k]) * t;
  const xTicks: ProfileTick[] = [];
  for (let share = 0; share <= span + 1e-9; share += 0.25)
    xTicks.push({ at: xOf(box, span, share), label: formatPercent(share) });
  return {
    box,
    mean: line(at(p.mean)),
    band: `${line([...upper, ...lower])}Z`,
    peak: { x: xOf(box, span, p.peak_at), y: yOf(box, peakValue) },
    end: xOf(box, span, 1),
    xTicks,
    yTicks: [0, 0.25, 0.5, 0.75, 1].map((v) => ({ at: yOf(box, v), label: formatPercent(v) })),
    summary:
      `Flick speed tops out at ${formatPercent(p.peak_at)} of the flick; ` +
      `braking takes ${formatPercent(p.braking)}.`,
    span,
  };
}

/**
 * One kill's own curve on the chart: its camera speed as a share of its top speed in the main flick, against its time
 * as a share of the flick, up to the plot's right edge. Null when the kill has no speed curve.
 */
export function killCurve(m: Flick | null, chart: ProfileModel): string | null {
  const c = m?.speed;
  if (!c || c.flick_end < 1 || c.speeds.length <= c.flick_end) return null;
  const top = Math.max(...c.speeds.slice(0, c.flick_end + 1));
  if (top <= 0) return null;
  const points = c.speeds
    .map((v, i) => ({ share: i / c.flick_end, v: v / top }))
    .filter((q) => q.share <= chart.span + 1e-9)
    .map((q) => ({ x: xOf(chart.box, chart.span, q.share), y: yOf(chart.box, q.v) }));
  return line(points);
}
