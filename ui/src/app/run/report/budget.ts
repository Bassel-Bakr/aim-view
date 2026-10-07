/**
 * Where a kill's time goes: its steps (Reaction, Flick, Micro, Confirmation, and a forced Reload)
 * as bars and a legend.
 *
 * In: the clicking summary's average kill (`budget`, the core's `KillParts`), a picked flick's own
 * parts and forced reload time, and the run's reloads.
 * Out: the "Where the time goes" panel's bars and legend (click-side), the kill parts and their
 * colors that the run charts share, and a kill's micro for the cards and the kills table.
 */

import { ClickSummary, Flick, KillParts } from '../../api';
import { formatMs } from '../../format';

/** A step of a kill: its name and the series token it is drawn in. */
export interface KillStep {
  /** The step's name, as shown. */
  label: string;
  /** The CSS color it is drawn in: a series token. */
  color: string;
}

/**
 * A kill's five measured parts, in order, and the series token each one is drawn in. The micro's
 * two parts (onto the target, then settling) share its color.
 */
export const PARTS: KillStep[] = [
  { label: 'Reaction', color: 'var(--series-1)' },
  { label: 'Flick', color: 'var(--series-2)' },
  { label: 'Micro onto the target', color: 'var(--series-3)' },
  { label: 'Micro settling', color: 'var(--series-3)' },
  { label: 'Confirmation', color: 'var(--series-5)' },
];

/**
 * The four steps a kill's time is shown in: Reaction, Flick, Micro (onto the target and settling),
 * Confirmation.
 */
export const STEPS: KillStep[] = [
  PARTS[0],
  PARTS[1],
  { label: 'Micro', color: 'var(--series-3)' },
  PARTS[4],
];

/**
 * The fifth step, in a scenario whose magazine runs out: the time a forced reload overlapped the
 * kill.
 */
export const RELOAD: KillStep = { label: 'Reload', color: 'var(--series-4)' };

/** The micro's index among the steps. */
const MICRO = 2;
/** The confirmation's index among the steps. */
const CONFIRMATION = 3;
/** The reload's index among the steps, when it is shown. */
const RELOAD_STEP = 4;

/** A kill's time in its four shown steps, in seconds. */
export type KillSteps = [reaction: number, flick: number, micro: number, confirmation: number];

/** A kill's five parts as its four steps: onto the target and settling make the micro. */
export function killSteps(parts: KillParts): KillSteps {
  return [parts[0], parts[1], parts[2] + parts[3], parts[4]];
}

/**
 * How a kill's forced reload is shown: the reload's time, and what it took from the confirmation
 * and the micro it overlapped. It comes out of the confirmation first, then the micro, never below
 * zero, so the kill's time stays its TTK; `shown` is the part that fits.
 */
interface ReloadShare {
  /** The forced reload's whole time, in seconds. */
  reload: number;
  /** The part of it shown as the reload step, in seconds: what the confirmation and micro held. */
  shown: number;
  /** The seconds taken out of the confirmation. */
  fromConfirmation: number;
  /** The seconds taken out of the micro. */
  fromMicro: number;
}

/** A forced reload of `reload` seconds, taken out of the kill's confirmation, then its micro. */
function reloadShare(parts: KillParts, reload: number): ReloadShare {
  const steps = killSteps(parts);
  const fromConfirmation = Math.min(reload, steps[CONFIRMATION]);
  const fromMicro = Math.min(reload - fromConfirmation, steps[MICRO]);
  return { reload, shown: fromConfirmation + fromMicro, fromConfirmation, fromMicro };
}

/**
 * The steps shown: the four, and the reload with them when the scenario's magazine runs out (reload
 * not null).
 */
function shownSteps(reload: number | null): KillStep[] {
  return reload == null ? STEPS : [...STEPS, RELOAD];
}

/**
 * A kill's time in the steps shown, in seconds: the four, or five with the reload taken out of the
 * others.
 */
function shownTimes(parts: KillParts, reload: number | null): number[] {
  const steps = killSteps(parts);
  if (reload == null) return steps;
  const share = reloadShare(parts, reload);
  return [
    steps[0],
    steps[1],
    steps[MICRO] - share.fromMicro,
    steps[CONFIRMATION] - share.fromConfirmation,
    share.shown,
  ];
}

/**
 * A kill's micro (onto the target, then settling), in seconds; null when its steps were not found.
 */
export function micro(flick: Flick): number | null {
  return flick.parts ? flick.parts[2] + flick.parts[3] : null;
}

/** The micro's two parts in words: "120 ms onto the target, 80 ms settling". */
export function microSplit(parts: KillParts): string {
  return `${formatMs(parts[2])} onto the target, ${formatMs(parts[3])} settling`;
}

/**
 * A step's name and time; the micro's adds its two parts: "Micro 200 ms: 120 ms onto the target,
 * 80 ms settling". With a forced reload, the micro and the confirmation say what the reload took
 * from them, and the reload says where its time came from. `i` is the step's index among those
 * shown.
 */
function stepTitle(parts: KillParts, i: number, reload: number | null): string {
  const title = `${shownSteps(reload)[i].label} ${formatMs(shownTimes(parts, reload)[i])}`;
  const share = reload == null ? null : reloadShare(parts, reload);
  const under = (taken: number) => (taken > 0 ? `, less ${formatMs(taken)} under the reload` : '');
  if (i === MICRO) return `${title}: ${microSplit(parts)}${under(share?.fromMicro ?? 0)}`;
  if (share && i === CONFIRMATION && share.fromConfirmation > 0)
    return `${title}: ${formatMs(killSteps(parts)[CONFIRMATION])}${under(share.fromConfirmation)}`;
  if (share && i === RELOAD_STEP) {
    const time =
      share.shown < share.reload
        ? `${formatMs(share.reload)}, ${formatMs(share.shown)} of it shown`
        : formatMs(share.shown);
    return `Reload ${time} (taken from the confirmation and micro it overlapped)`;
  }
  return title;
}

/** A step's share of a bar. text: its name and time, shown inside the bar when there is room. */
export interface BudgetSegment {
  /** The step's color. */
  color: string;
  /** The step's share of the bar's time, 0 to 1 (its flex grow). */
  grow: number;
  /** The step's name and time in full, on hover. */
  title: string;
  /** The step's name and time inside the bar; '' when it is too narrow or the bar is thin. */
  text: string;
}

/**
 * One bar: a kill's steps side by side, as wide as its time against the longest bar (a share of
 * 100).
 */
export interface BudgetBar {
  /** The text beside a reference bar (the average kill); null for the main bar. */
  label: string | null;
  /** The bar's width, in percent of the panel. */
  width: number;
  /** A thin reference bar, with no text inside. */
  thin: boolean;
  /**
   * Meant to keep its height only, so the box does not jump when a kill is picked; the template
   * (click-side.html) draws nothing for it.
   */
  hidden: boolean;
  /** The steps, in order. */
  segments: BudgetSegment[];
}

/**
 * A step in the legend: its time, and the average kill's beside it. title: the micro's two parts.
 */
export interface BudgetLegendItem {
  /** The step's color. */
  color: string;
  /** The step's name and time. */
  text: string;
  /** The step's name and time in full, on hover. */
  title: string;
  /** The average kill's time for the step, "(avg 120 ms)"; null with no average shown. */
  average: string | null;
  /** The average is not shown: the bars are the average kill already. */
  averageHidden: boolean;
}

/** The "Where the time goes" panel: its title, a note, the bars and the legend. */
export interface Budget {
  /** The panel's title, with the kill's TTK. */
  title: string;
  /** Why the picked kill's time cannot be split; null otherwise. */
  note: string | null;
  /** The bars: the kill's, then the average's for reference. */
  bars: BudgetBar[];
  /** One item for each step shown. */
  legend: BudgetLegendItem[];
}

/** A step is named inside its bar when it fills at least this share of it. */
const LABEL_SHARE = 0.12;

/** A kill's whole time from its parts, in seconds. */
const sum = (parts: KillParts) => parts.reduce((a, b) => a + b, 0);

/**
 * A kill's parts and its forced reload's time (null: the scenario's magazine never runs out, no
 * reload step).
 */
interface KillTime {
  /** The kill's five parts, in seconds. */
  parts: KillParts;
  /** Its forced reload's time, in seconds; null with no reload step. */
  reload: number | null;
}

/**
 * A kill's bar, `width` percent wide, with its steps as segments; `label` names a reference bar,
 * `thin` draws it thin with no text, `hidden` marks it not to draw.
 */
function bar(
  { parts, reload }: KillTime,
  width: number,
  label: string | null,
  thin = false,
  hidden = false,
): BudgetBar {
  const total = sum(parts);
  const steps = shownSteps(reload);
  return {
    label,
    width,
    thin,
    hidden,
    segments: shownTimes(parts, reload).map((seconds, i) => ({
      color: steps[i].color,
      grow: total ? seconds / total : 0,
      title: stepTitle(parts, i, reload),
      text:
        !thin && total && seconds / total > LABEL_SHARE
          ? `${steps[i].label} ${formatMs(seconds)}`
          : '',
    })),
  };
}

/**
 * The legend: each step shown with the kill's time, and the average kill's beside it when given
 * (`averageHidden` marks it not to show).
 */
function legend(
  kill: KillTime,
  average: KillTime | null,
  averageHidden = false,
): BudgetLegendItem[] {
  const steps = shownSteps(kill.reload);
  const averages = average && shownTimes(average.parts, average.reload);
  return shownTimes(kill.parts, kill.reload).map((seconds, i) => ({
    color: steps[i].color,
    text: `${steps[i].label} ${formatMs(seconds)}`,
    title: stepTitle(kill.parts, i, kill.reload),
    average: averages ? `(avg ${formatMs(averages[i])})` : null,
    averageHidden,
  }));
}

/**
 * The average kill's forced reload time in seconds, over the kills whose steps were found (as the
 * summary's budget averages them); null when the scenario's magazine never runs out (the report has
 * no reloads).
 */
export function averageReload(summary: ClickSummary, flicks: Flick[]): number | null {
  if (!summary.reloads) return null;
  const split = flicks.filter((flick) => flick.parts);
  return split.length
    ? split.reduce((total, flick) => total + (flick.reload_time ?? 0), 0) / split.length
    : 0;
}

/**
 * Where the time goes: the run's average kill, or a picked kill with the average under it on the
 * same time scale. A kill with a step that was not found shows the average with a note. `reload`:
 * the average kill's forced reload time (averageReload), which adds the reload step; null when the
 * scenario's magazine never runs out. Null when the summary has no average kill.
 */
export function budget(
  averageParts: KillParts | null,
  flick: Flick | null,
  reload: number | null = null,
): Budget | null {
  if (!averageParts) return null;
  const average: KillTime = { parts: averageParts, reload };
  if (!flick) {
    return {
      title: `Where an average kill's ${formatMs(sum(averageParts))} goes`,
      note: null,
      bars: [bar(average, 100, null), bar(average, 100, 'Average kill', true, true)],
      legend: legend(average, average, true),
    };
  }
  const title = `Where kill ${flick.kill_number}'s ${formatMs(flick.total)} goes`;
  if (!flick.parts) {
    return {
      title,
      note: "One of this kill's steps (the reaction, the arrival or the confirmation) was not found, so its time can't be split. The run's average:",
      bars: [bar(average, 100, null)],
      legend: legend(average, null),
    };
  }
  const kill: KillTime = {
    parts: flick.parts,
    reload: reload == null ? null : (flick.reload_time ?? 0),
  };
  const top = Math.max(sum(flick.parts), sum(averageParts));
  return {
    title,
    note: null,
    bars: [
      bar(kill, (100 * sum(flick.parts)) / top, null),
      bar(
        average,
        (100 * sum(averageParts)) / top,
        `Average kill, ${formatMs(sum(averageParts))}`,
        true,
      ),
    ],
    legend: legend(kill, average),
  };
}
