/**
 * The progress chart's SVG shapes, from its layout: every past run's dot as one path, and the median as one line.
 *
 * In: the chart's layout (progress-chart-model.ts `progressChart`) and the dots' sizes from the tokens (tokens.ts, from
 * themes/progress-chart.scss; the same in both themes). Out: the paths and sizes progress-chart.html draws; their
 * colors and line widths are the stylesheet's (progress-chart.scss).
 */

import { TOKENS } from '../../tokens/tokens';
import { HistoryModel, HistoryPoint } from './progress-chart-model';

/** A run dot's radius, in CSS pixels. */
export const DOT_RADIUS = Number(TOKENS.dark['progress-chart-dot']);
/** This run's dot's radius, in CSS pixels. */
export const CURRENT_RADIUS = Number(TOKENS.dark['progress-chart-current']);
/** The personal best's ring's radius, in CSS pixels. */
export const RING_RADIUS = Number(TOKENS.dark['progress-chart-ring']);
/** How near a dot the pointer must be to pick it, in CSS pixels. */
export const PICK_REACH = Number(TOKENS.dark['progress-chart-reach']);
/** Decimal places kept in a path's numbers: a tenth of a pixel. */
const PLACES = 1;

/** A number in a path, rounded to a tenth of a pixel. */
function round(value: number): number {
  return Number(value.toFixed(PLACES));
}

/** Circles of radius `radiusPx` at the points, as one path (two arcs each). */
export function circlesPath(points: readonly HistoryPoint[], radiusPx: number): string {
  return points
    .map(
      ({ x, y }) =>
        `M${round(x - radiusPx)} ${round(y)}a${radiusPx} ${radiusPx} 0 1 0 ${2 * radiusPx} 0` +
        `a${radiusPx} ${radiusPx} 0 1 0 ${-2 * radiusPx} 0`,
    )
    .join('');
}

/** A line through the points, in their order. */
export function linePath(points: readonly HistoryPoint[]): string {
  return points.map(({ x, y }, i) => `${i ? 'L' : 'M'}${round(x)} ${round(y)}`).join('');
}

/** The chart's two paths (every run's dot, and the median line) with the layout they come from. */
export interface ChartPaths {
  /** The chart's layout. */
  model: HistoryModel;
  /** Every past run's dot. */
  runs: string;
  /** The median line through the runs. */
  median: string;
}

/** The chart's paths from its layout. */
export function chartPaths(model: HistoryModel): ChartPaths {
  return { model, runs: circlesPath(model.dots, DOT_RADIUS), median: linePath(model.median) };
}
