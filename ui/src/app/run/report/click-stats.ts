/**
 * A clicking run's numbers, in words for the page.
 *
 * In: the clicking summary, flicks and checks the core works out (src/summary.rs `Summary`,
 * `Issue`; src/matching.rs `Flick`), and the fastest-path summary.
 * Out: the number cards for the run or a kill, the headline tiles, the source note, the
 * by-distance and by-direction rows, the TTK by distance bars and the what-if table.
 */

import {
  ClickSummary,
  ClickWhatIf,
  ClickWhatIfGroup,
  DirectionBand,
  DistanceBand,
  Flick,
  Issue,
  Reloads,
} from '../../api';
import { WhatIfTable } from '../what-if-section/what-if-section';
import {
  arrow,
  DIRECTION_ARROWS,
  formatCount,
  formatDegrees,
  formatEnded,
  formatMs,
  formatPercent,
  formatSeconds,
  formatSpeed,
} from '../../format';
import { median } from '../median';
import { micro, microSplit } from './budget';

/**
 * A number card: its value, what it is, and a line under it. title: more about the value, on
 * hover.
 */
export interface Stat {
  /** What the number is. */
  label: string;
  /** The number, formatted. */
  value: string;
  /** A line under the number: the run's median beside a kill's, or a note; '' for none. */
  detail: string;
  /** More about the value, on hover; the value itself when absent. */
  title?: string;
}

/** The run's picks against the fastest order, for its cards: null while the tracks load. */
export interface PathSummary {
  /** The share of picks with a choice that were the fastest; null when no pick had a choice. */
  share: number | null;
  /** The time the picks lost in all, in seconds. */
  total: number;
  /** More shots (kills without a stats file) the best picks would have given, at the run's pace. */
  extra: number;
}

/** A path card's value while the tracks load. */
const LOADING = '…';

/**
 * Kills a minute, from the first flick's start to the last kill; null with fewer than two kills (or
 * no time between them).
 */
export function killsPerMinute(flicks: Flick[], fps: number): number | null {
  if (flicks.length < 2) return null;
  const seconds = (flicks[flicks.length - 1].kill_frame - flicks[0].start_frame) / fps;
  return seconds > 0 ? (60 * flicks.length) / seconds : null;
}

/** The run's median of one of the flicks' measures, leaving out the flicks without it. */
function runMedian(flicks: Flick[], measure: (m: Flick) => number | null): number | null {
  return median(flicks.map(measure).filter((value): value is number => value != null));
}

/**
 * The run's forced reloads: how many, their time, and its share of the run (from the first flick to
 * the last kill). Reloads the player chose don't show in the stats.
 */
function reloadsCard(reloads: Reloads, flicks: Flick[], fps: number): Stat {
  const span =
    flicks.length >= 2 ? (flicks[flicks.length - 1].kill_frame - flicks[0].start_frame) / fps : 0;
  const share = span > 0 ? ` · ${formatPercent(reloads.seconds / span)} of the run` : '';
  const points = reloads.score_lost
    ? ` They took ${formatCount(reloads.score_lost)} points off.`
    : '';
  return {
    label: 'Reloads',
    value: formatCount(reloads.count),
    detail: `${formatSeconds(reloads.seconds)}${share}`,
    title: `Reloads an empty magazine forced; reloads you chose don't show in the stats.${points}`,
  };
}

/** A kill's forced reload: its time, or none. */
function reloadCard(flick: Flick): Stat {
  const count = flick.reloads ?? 0;
  return {
    label: 'Reload',
    value: count ? formatMs(flick.reload_time) : 'none',
    detail: count > 1 ? `${count} forced reloads` : count ? 'forced by an empty magazine' : '',
  };
}

/**
 * The whole run's cards: the score and kills, then the times and speeds, then the path; sixteen, as
 * many as a kill has, so the rows stay put when a kill is picked. In a scenario whose magazine runs
 * out, both add the reloads.
 */
export function runStats(
  summary: ClickSummary,
  paths: PathSummary | null,
  flicks: Flick[] = [],
  fps = 60,
): Stat[] {
  const pace = killsPerMinute(flicks, fps);
  return [
    { label: 'Score', value: formatCount(summary.score), detail: '' },
    { label: 'Kills', value: formatCount(summary.kills), detail: '' },
    { label: 'Misses', value: formatCount(summary.misses), detail: '' },
    { label: 'Median TTK', value: formatMs(summary.median_interval), detail: 'time to kill' },
    { label: 'Confirmation', value: formatMs(summary.still), detail: 'still before the click' },
    { label: 'Flick speed', value: formatSpeed(summary.peak), detail: '' },
    {
      label: 'Game FPS',
      value: summary.fps_avg ? String(Math.round(summary.fps_avg)) : '–',
      detail: '',
    },
    {
      label: 'Fastest next target',
      value: paths ? formatPercent(paths.share) : LOADING,
      detail: 'of the picks with a choice',
    },
    { label: 'Pathing in all', value: paths ? formatMs(paths.total) : LOADING, detail: '' },
    {
      label: `More ${summary.shots == null ? 'kills' : 'shots'} with the best path`,
      value: paths ? `≈ ${paths.extra.toFixed(1)}` : LOADING,
      detail: 'at your pace',
    },
    { label: 'Accuracy', value: formatPercent(summary.accuracy), detail: '' },
    { label: 'Reaction', value: formatMs(summary.react), detail: 'median' },
    { label: 'Flick', value: formatMs(summary.flick), detail: 'median' },
    { label: 'Click on the move', value: formatSpeed(summary.click_speed), detail: 'median' },
    { label: 'Click off center', value: formatDegrees(summary.click_off), detail: 'median' },
    {
      label: 'Kills a minute',
      value: pace == null ? '–' : pace.toFixed(1),
      detail: 'first flick to last kill',
    },
    ...(summary.reloads ? [reloadsCard(summary.reloads, flicks, fps)] : []),
  ];
}

/**
 * One kill's cards, each with the run's median under it where there is one. `pathCost` is the
 * kill's Pathing card value, already in words.
 */
export function killStats(
  flick: Flick,
  summary: ClickSummary,
  pathCost: string,
  flicks: Flick[] = [],
): Stat[] {
  const run = (value: string) => `run ${value}`;
  const micros = runMedian(flicks, (other) => other.corrections);
  return [
    { label: 'Distance', value: `${flick.D0.toFixed(1)}°`, detail: '' },
    { label: 'Toward', value: arrow(flick.direction_deg), detail: '' },
    { label: 'TTK', value: formatMs(flick.total), detail: run(formatMs(summary.median_interval)) },
    { label: 'Reaction', value: formatMs(flick.react), detail: run(formatMs(summary.react)) },
    { label: 'Flick', value: formatMs(flick.flick), detail: run(formatMs(summary.flick)) },
    { label: 'Flick landed', value: formatEnded(flick.end_left, summary.radius), detail: '' },
    { label: 'Confirmation', value: formatMs(flick.still), detail: run(formatMs(summary.still)) },
    {
      label: 'Flick speed',
      value: formatSpeed(flick.peak),
      detail: run(formatSpeed(summary.peak)),
    },
    {
      label: 'Click on the move',
      value: formatSpeed(flick.click_speed),
      detail: run(formatSpeed(summary.click_speed)),
    },
    { label: 'Shots', value: formatCount(flick.shots), detail: '' },
    { label: 'Pathing', value: pathCost, detail: 'against the fastest pick' },
    {
      label: 'Micro',
      value: formatMs(micro(flick)),
      detail: run(formatMs(runMedian(flicks, micro))),
      title: flick.parts ? microSplit(flick.parts) : undefined,
    },
    {
      label: 'Click off center',
      value: formatDegrees(flick.click_off),
      detail: run(formatDegrees(summary.click_off)),
    },
    {
      label: 'Micros',
      value: formatCount(flick.corrections),
      detail: micros == null ? '' : run(String(micros)),
    },
    {
      label: 'Went past by',
      value: formatDegrees(Math.max(0, flick.past - summary.radius)),
      detail: 'past the far edge',
    },
    {
      label: 'Spawn',
      value: flick.spawned ? 'yes' : 'no',
      detail: 'it appeared after the flick began',
    },
    ...(summary.reloads ? [reloadCard(flick)] : []),
  ];
}

/** Where the run's kills came from, and what was measured. */
export function sourceNote(summary: ClickSummary): string {
  const { source, matched, kills_stats: counted } = summary.info;
  let from: string;
  if (source === 'video') {
    from =
      `No stats file and no readable session HUD: ${matched} kills found in the video alone (in tests between 1 in 3 ` +
      'and 9 in 10 are right, depending on the run). These are kills, not shots: a target that takes two hits is one ' +
      'kill, and score, misses, accuracy and shot counts are not available';
  } else if (source === 'hud') {
    from = `No stats file: kills, shots and hits read from KovaaK's session HUD in the video (${matched} of ${counted} kills matched to a target); the score is the file name's`;
  } else if (source === 'aimlab') {
    from =
      `No stats file: hits and misses read from Aim Lab's POINTS in the video (a hit adds points, a miss takes some ` +
      `off; every hit is counted as a kill), ${matched} of ${counted} matched to a target; the score is the last POINTS`;
  } else {
    from = `${matched} of ${counted} kills matched with the stats file`;
  }
  const sens = summary.sens ? ` · ${summary.sens}` : '';
  return `${from} · ${summary.measured} flicks measured · target radius ${summary.radius.toFixed(2)}°${sens}`;
}

/**
 * A headline tile: a number, what it is, a line under it, whether the review flags it, whether the
 * line is good news (a better score than the run before), and what it means.
 */
export interface HeadlineTile {
  /** What the number is. */
  label: string;
  /** The number, formatted. */
  value: string;
  /** The line under the number; on the score's tile, the change from the run before. */
  note: string;
  /** The review flags the tile's check: work on this. */
  attention: boolean;
  /** The note is good news: a better score than the run before. */
  good: boolean;
  /** What the number means, on hover; '' for a clicking run's tiles. */
  why: string;
}

/** The Accuracy tile's check: the misses' share of the shots (review.py's issue number). */
const ISSUE_MISSES = 39;
/** The Confirmation tile's check: the wait before the click against the TTK (review.py's issue). */
const ISSUE_WAITING = 36;
/** The Reaction tile's check: the median reaction after a kill (review.py's issue number). */
const ISSUE_START = 1;
/** The Median TTK tile's check: the last third's TTK against the first's (review.py's issue). */
const ISSUE_PACE = 49;

/** The run in six numbers, above the video; a tile is flagged when the review flags its check. */
export function runHeadline(
  summary: ClickSummary,
  issues: Issue[],
  flicks: Flick[] = [],
  fps = 60,
): HeadlineTile[] {
  const flagged = (issueNumber: number) =>
    issues.some((i) => i.issue === issueNumber && i.flag === 'attention');
  const pace = killsPerMinute(flicks, fps);
  const { matched, kills_stats: counted } = summary.info;
  const stillShare =
    summary.still != null && summary.median_interval
      ? summary.still / summary.median_interval
      : null;
  const tile = (label: string, value: string, note: string, issue = 0): HeadlineTile => ({
    label,
    value,
    note,
    attention: issue > 0 && flagged(issue),
    good: false,
    why: '',
  });
  return [
    tile('Score', formatCount(summary.score), summary.sens ?? ''),
    tile(
      'Kills',
      formatCount(summary.kills),
      matched != null && counted != null ? `${matched} of ${counted} found in the video` : '',
    ),
    tile(
      'Accuracy',
      formatPercent(summary.accuracy),
      summary.misses == null ? '' : `${formatCount(summary.misses)} misses`,
      ISSUE_MISSES,
    ),
    tile(
      'Median TTK',
      formatMs(summary.median_interval),
      pace == null ? '' : `${pace.toFixed(1)} kills a minute`,
      ISSUE_PACE,
    ),
    tile(
      'Confirmation',
      formatMs(summary.still),
      stillShare == null ? '' : `${formatPercent(stillShare)} of a kill`,
      ISSUE_WAITING,
    ),
    tile('Reaction', formatMs(summary.react), 'after each kill, median', ISSUE_START),
  ];
}

/**
 * A bar of the kill time by distance: the band, its median kill, how often it stopped short, and
 * its length.
 */
export interface DistanceBar {
  /** The band's distances, "10–20°", or "60°+" for the widest. */
  band: string;
  /** The band's median TTK. */
  kill: string;
  /** The band's share of underflicks. */
  short: string;
  /** The median kill against the slowest band's, 0 to 1. */
  share: number;
}

/** The TTK by distance bars beside the video, one per band, as long as its median TTK. */
export function distanceBars(bands: DistanceBand[]): DistanceBar[] {
  const longest = Math.max(...bands.map((b) => b.interval ?? 0));
  return distanceRows(bands).map((row, i) => ({
    band: bands[i].hi === OPEN_BAND ? `${bands[i].lo}°+` : row.band,
    kill: row.kill,
    short: row.short,
    share: longest > 0 ? (bands[i].interval ?? 0) / longest : 0,
  }));
}

/**
 * The checks, those that need attention first, each group in its own order (the core's, and the
 * page's own Pathing check, which has no issue number).
 */
export function sortedIssues(issues: Omit<Issue, 'issue'>[]): Omit<Issue, 'issue'>[] {
  return [...issues].sort(
    (a, b) => Number(b.flag === 'attention') - Number(a.flag === 'attention'),
  );
}

/** A row of the by-distance table, as shown. */
export interface DistanceRow {
  /** The band's distances, "10–20°", or "60° and over" for the widest. */
  band: string;
  /** How many flicks the band has. */
  flicks: number;
  /** The band's median TTK. */
  kill: string;
  /** The band's median reaction. */
  reaction: string;
  /** The band's share of underflicks. */
  short: string;
  /** The band's share of overflicks. */
  past: string;
  /** The band's median confirmation. */
  still: string;
}

/** The widest band ends at 90°: it reads "60° and over". */
const OPEN_BAND = 90;

/** The by-distance table's rows, one per band. */
export function distanceRows(bands: DistanceBand[]): DistanceRow[] {
  return bands.map((b) => ({
    band: b.hi === OPEN_BAND ? `${b.lo}° and over` : `${b.lo}–${b.hi}°`,
    flicks: b.n,
    kill: formatMs(b.interval),
    reaction: formatMs(b.react),
    short: formatPercent(b.short),
    past: formatPercent(b.past),
    still: formatMs(b.still),
  }));
}

/** A row of the by-direction table, as shown. */
export interface DirectionRow {
  /** The direction, an arrow and its name. */
  toward: string;
  /** How many flicks went this way. */
  flicks: number;
  /** Their median TTK. */
  kill: string;
  /** Their median distance. */
  distance: string;
  /** Their median time beyond what the distance predicts, signed. */
  beyond: string;
  /** Their share of underflicks. */
  short: string;
  /** Their share of overflicks. */
  past: string;
}

/** The by-direction table's rows, one per direction (a 45-degree sector). */
export function directionRows(bands: DirectionBand[]): DirectionRow[] {
  return bands.map((b) => ({
    toward: `${DIRECTION_ARROWS[b.name]} ${b.name}`,
    flicks: b.n,
    kill: formatMs(b.interval),
    distance: b.distance == null ? '–' : `${b.distance.toFixed(1)}°`,
    beyond: b.beyond == null ? '–' : `${b.beyond >= 0 ? '+' : '−'}${formatMs(Math.abs(b.beyond))}`,
    short: formatPercent(b.short),
    past: formatPercent(b.past),
  }));
}

/** A what-if group and its heading. */
type WhatIfHeading = [group: ClickWhatIfGroup, name: string];

/** The what-if groups in the order shown, with their headings. */
const WHAT_IF_GROUPS: WhatIfHeading[] = [
  ['pace', 'Pace'],
  ['flicks', 'Flicks'],
  ['micros', 'Micros'],
];

/** A gain with its sign: one decimal under 10, whole numbers from there. */
function plus(gain: number): string {
  return `+${Math.abs(gain) < 9.95 ? gain.toFixed(1) : gain.toFixed(0)}`;
}

/**
 * The what-if table: the extra kills each change would give, and the extra score where it is known,
 * under Pace, Flicks and Micros, each group biggest first. A group with no lines is left out, and
 * so is the table on reports that lack the lines (older cores).
 */
export function clickWhatIf(whatIfs: ClickWhatIf[] | undefined): WhatIfTable {
  const lines = whatIfs ?? [];
  const score = lines.some((line) => line.score !== null);
  const groups = WHAT_IF_GROUPS.map(([group, name]) => ({
    name,
    lines: lines
      .filter((line) => line.group === group)
      .sort((a, b) => b.kills - a.kills)
      .map((line) => ({
        what: line.what,
        gains: score
          ? [`${plus(line.kills)} kills`, line.score === null ? '' : `${plus(line.score)} score`]
          : [`${plus(line.kills)} kills`],
        how: line.how,
      })),
  })).filter((group) => group.lines.length);
  return { columns: score ? ['Kills', 'Score'] : ['Kills'], groups };
}
