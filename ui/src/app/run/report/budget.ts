import { Flick, KillParts } from '../../api';
import { formatMs } from '../../format';

/** The five steps of a kill, in order, and the series token each one is drawn in. */
export const PARTS = [
  { label: 'React', color: 'var(--series-1)' },
  { label: 'Main flick', color: 'var(--series-2)' },
  { label: 'Onto the target', color: 'var(--series-3)' },
  { label: 'Settle', color: 'var(--series-4)' },
  { label: 'Still on the target', color: 'var(--series-5)' },
];

/** A step's share of a bar. text: its name and time, shown inside the bar when there is room. */
export interface BudgetSegment {
  color: string;
  grow: number;
  title: string;
  text: string;
}

/** One bar: a kill's steps side by side, as wide as its time against the longest bar (a share of 100). */
export interface BudgetBar {
  label: string | null;
  width: number;
  thin: boolean;
  /** Kept for its height only, so the box does not jump when a kill is picked. */
  hidden: boolean;
  segments: BudgetSegment[];
}

/** A step in the legend: its time, and the average kill's beside it. */
export interface BudgetLegendItem {
  color: string;
  text: string;
  average: string | null;
  averageHidden: boolean;
}

export interface Budget {
  title: string;
  note: string | null;
  bars: BudgetBar[];
  legend: BudgetLegendItem[];
}

/** A step is named inside its bar when it fills at least this share of it. */
const LABEL_SHARE = 0.12;

const sum = (p: KillParts) => p.reduce((a, b) => a + b, 0);

function bar(
  parts: KillParts,
  width: number,
  label: string | null,
  thin = false,
  hidden = false,
): BudgetBar {
  const total = sum(parts);
  return {
    label,
    width,
    thin,
    hidden,
    segments: parts.map((v, i) => ({
      color: PARTS[i].color,
      grow: total ? v / total : 0,
      title: `${PARTS[i].label}: ${formatMs(v)}`,
      text: !thin && total && v / total > LABEL_SHARE ? `${PARTS[i].label} ${formatMs(v)}` : '',
    })),
  };
}

function legend(
  parts: KillParts,
  average: KillParts | null,
  averageHidden = false,
): BudgetLegendItem[] {
  return parts.map((v, i) => ({
    color: PARTS[i].color,
    text: `${PARTS[i].label} ${formatMs(v)}`,
    average: average ? `(avg ${formatMs(average[i])})` : null,
    averageHidden,
  }));
}

/**
 * Where the time goes: the run's average kill, or a picked kill with the average under it on the same time scale. A
 * kill with a step that was not found shows the average with a note.
 */
export function budget(average: KillParts | null, flick: Flick | null): Budget | null {
  if (!average) return null;
  if (!flick) {
    return {
      title: `Where an average kill's ${formatMs(sum(average))} goes`,
      note: null,
      bars: [bar(average, 100, null), bar(average, 100, 'Average kill', true, true)],
      legend: legend(average, average, true),
    };
  }
  const title = `Where kill ${flick.n}'s ${formatMs(flick.total)} goes`;
  if (!flick.parts) {
    return {
      title,
      note: "One of this kill's steps (the reaction, the arrival or the stop on the target) was not found, so its time can't be split. The run's average:",
      bars: [bar(average, 100, null)],
      legend: legend(average, null),
    };
  }
  const top = Math.max(sum(flick.parts), sum(average));
  return {
    title,
    note: null,
    bars: [
      bar(flick.parts, (100 * sum(flick.parts)) / top, null),
      bar(average, (100 * sum(average)) / top, `Average kill, ${formatMs(sum(average))}`, true),
    ],
    legend: legend(flick.parts, average),
  };
}
