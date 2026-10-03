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

/** A number card: its value, what it is, and a line under it. title: more about the value, on hover. */
export interface Stat {
  label: string;
  value: string;
  detail: string;
  title?: string;
}

/** The run's picks against the fastest order, for its cards: null while the tracks load. */
export interface PathSummary {
  share: number | null;
  total: number;
  /** More shots (kills without a stats file) the best picks would have given, at the run's pace. */
  extra: number;
}

const LOADING = '…';

/** Kills a minute, from the first flick's start to the last kill; null with fewer than two kills. */
export function killsPerMinute(flicks: Flick[], fps: number): number | null {
  if (flicks.length < 2) return null;
  const seconds = (flicks[flicks.length - 1].kill_frame - flicks[0].start_frame) / fps;
  return seconds > 0 ? (60 * flicks.length) / seconds : null;
}

/** The run's median of one of the flicks' measures, leaving out the flicks without it. */
function runMedian(flicks: Flick[], measure: (m: Flick) => number | null): number | null {
  return median(flicks.map(measure).filter((v): v is number => v != null));
}

/**
 * The run's forced reloads: how many, their time, and its share of the run (from the first flick to the last kill).
 * Reloads the player chose don't show in the stats.
 */
function reloadsCard(r: Reloads, flicks: Flick[], fps: number): Stat {
  const span =
    flicks.length >= 2 ? (flicks[flicks.length - 1].kill_frame - flicks[0].start_frame) / fps : 0;
  const share = span > 0 ? ` · ${formatPercent(r.seconds / span)} of the run` : '';
  const points = r.score_lost ? ` They took ${formatCount(r.score_lost)} points off.` : '';
  return {
    label: 'Reloads',
    value: formatCount(r.count),
    detail: `${formatSeconds(r.seconds)}${share}`,
    title: `Reloads an empty magazine forced; reloads you chose don't show in the stats.${points}`,
  };
}

/** A kill's forced reload: its time, or none. */
function reloadCard(m: Flick): Stat {
  const n = m.reloads ?? 0;
  return {
    label: 'Reload',
    value: n ? formatMs(m.reload_time) : 'none',
    detail: n > 1 ? `${n} forced reloads` : n ? 'forced by an empty magazine' : '',
  };
}

/**
 * The whole run's cards: the score and kills, then the times and speeds, then the path; sixteen, as many as a kill
 * has, so the rows stay put when a kill is picked. In a scenario whose magazine runs out, both add the reloads.
 */
export function runStats(
  s: ClickSummary,
  paths: PathSummary | null,
  flicks: Flick[] = [],
  fps = 60,
): Stat[] {
  const pace = killsPerMinute(flicks, fps);
  return [
    { label: 'Score', value: formatCount(s.score), detail: '' },
    { label: 'Kills', value: formatCount(s.kills), detail: '' },
    { label: 'Misses', value: formatCount(s.misses), detail: '' },
    { label: 'Median TTK', value: formatMs(s.median_interval), detail: 'time to kill' },
    { label: 'Confirmation', value: formatMs(s.still), detail: 'still before the click' },
    { label: 'Flick speed', value: formatSpeed(s.peak), detail: '' },
    { label: 'Game FPS', value: s.fps_avg ? String(Math.round(s.fps_avg)) : '–', detail: '' },
    {
      label: 'Fastest next target',
      value: paths ? formatPercent(paths.share) : LOADING,
      detail: 'of the picks with a choice',
    },
    { label: 'Pathing in all', value: paths ? formatMs(paths.total) : LOADING, detail: '' },
    {
      label: `More ${s.shots == null ? 'kills' : 'shots'} with the best path`,
      value: paths ? `≈ ${paths.extra.toFixed(1)}` : LOADING,
      detail: 'at your pace',
    },
    { label: 'Accuracy', value: formatPercent(s.accuracy), detail: '' },
    { label: 'Reaction', value: formatMs(s.react), detail: 'median' },
    { label: 'Flick', value: formatMs(s.flick), detail: 'median' },
    { label: 'Click on the move', value: formatSpeed(s.click_speed), detail: 'median' },
    { label: 'Click off center', value: formatDegrees(s.click_off), detail: 'median' },
    {
      label: 'Kills a minute',
      value: pace == null ? '–' : pace.toFixed(1),
      detail: 'first flick to last kill',
    },
    ...(s.reloads ? [reloadsCard(s.reloads, flicks, fps)] : []),
  ];
}

/** One kill's cards, each with the run's median under it where there is one. */
export function killStats(
  m: Flick,
  s: ClickSummary,
  pathCost: string,
  flicks: Flick[] = [],
): Stat[] {
  const run = (v: string) => `run ${v}`;
  const micros = runMedian(flicks, (f) => f.corr);
  return [
    { label: 'Distance', value: `${m.D0.toFixed(1)}°`, detail: '' },
    { label: 'Toward', value: arrow(m.dir), detail: '' },
    { label: 'TTK', value: formatMs(m.total), detail: run(formatMs(s.median_interval)) },
    { label: 'Reaction', value: formatMs(m.react), detail: run(formatMs(s.react)) },
    { label: 'Flick', value: formatMs(m.flick), detail: run(formatMs(s.flick)) },
    { label: 'Flick landed', value: formatEnded(m.end_left, s.radius), detail: '' },
    { label: 'Confirmation', value: formatMs(m.still), detail: run(formatMs(s.still)) },
    { label: 'Flick speed', value: formatSpeed(m.peak), detail: run(formatSpeed(s.peak)) },
    {
      label: 'Click on the move',
      value: formatSpeed(m.click_speed),
      detail: run(formatSpeed(s.click_speed)),
    },
    { label: 'Shots', value: formatCount(m.shots), detail: '' },
    { label: 'Pathing', value: pathCost, detail: 'against the fastest pick' },
    {
      label: 'Micro',
      value: formatMs(micro(m)),
      detail: run(formatMs(runMedian(flicks, micro))),
      title: m.parts ? microSplit(m.parts) : undefined,
    },
    {
      label: 'Click off center',
      value: formatDegrees(m.click_off),
      detail: run(formatDegrees(s.click_off)),
    },
    {
      label: 'Micros',
      value: formatCount(m.corr),
      detail: micros == null ? '' : run(String(micros)),
    },
    {
      label: 'Went past by',
      value: formatDegrees(Math.max(0, m.past - s.radius)),
      detail: 'past the far edge',
    },
    {
      label: 'Spawn',
      value: m.spawned ? 'yes' : 'no',
      detail: 'it appeared after the flick began',
    },
    ...(s.reloads ? [reloadCard(m)] : []),
  ];
}

/** Where the run's kills came from, and what was measured. */
export function sourceNote(s: ClickSummary): string {
  const { source, matched, kills_stats: counted } = s.info;
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
  const sens = s.sens ? ` · ${s.sens}` : '';
  return `${from} · ${s.measured} flicks measured · target radius ${s.radius.toFixed(2)}°${sens}`;
}

/**
 * A headline tile: a number, what it is, a line under it, whether the review flags it, whether the line is good news
 * (a better score than the run before), and what it means.
 */
export interface HeadlineTile {
  label: string;
  value: string;
  note: string;
  attention: boolean;
  good: boolean;
  why: string;
}

/** The checks behind the headline numbers (review.py's issue numbers). */
const ISSUE_MISSES = 39;
const ISSUE_WAITING = 36;
const ISSUE_START = 1;
const ISSUE_PACE = 49;

/** The run in six numbers, above the video; a tile is flagged when the review flags its check. */
export function runHeadline(
  s: ClickSummary,
  issues: Issue[],
  flicks: Flick[] = [],
  fps = 60,
): HeadlineTile[] {
  const flagged = (n: number) => issues.some((i) => i.issue === n && i.flag === 'attention');
  const pace = killsPerMinute(flicks, fps);
  const { matched, kills_stats: counted } = s.info;
  const stillShare = s.still != null && s.median_interval ? s.still / s.median_interval : null;
  const tile = (label: string, value: string, note: string, issue = 0): HeadlineTile => ({
    label,
    value,
    note,
    attention: issue > 0 && flagged(issue),
    good: false,
    why: '',
  });
  return [
    tile('Score', formatCount(s.score), s.sens ?? ''),
    tile(
      'Kills',
      formatCount(s.kills),
      matched != null && counted != null ? `${matched} of ${counted} found in the video` : '',
    ),
    tile(
      'Accuracy',
      formatPercent(s.accuracy),
      s.misses == null ? '' : `${formatCount(s.misses)} misses`,
      ISSUE_MISSES,
    ),
    tile(
      'Median TTK',
      formatMs(s.median_interval),
      pace == null ? '' : `${pace.toFixed(1)} kills a minute`,
      ISSUE_PACE,
    ),
    tile(
      'Confirmation',
      formatMs(s.still),
      stillShare == null ? '' : `${formatPercent(stillShare)} of a kill`,
      ISSUE_WAITING,
    ),
    tile('Reaction', formatMs(s.react), 'after each kill, median', ISSUE_START),
  ];
}

/** A bar of the kill time by distance: the band, its median kill, how often it stopped short, and its length. */
export interface DistanceBar {
  band: string;
  kill: string;
  short: string;
  /** The median kill against the slowest band's, 0 to 1. */
  share: number;
}

export function distanceBars(bands: DistanceBand[]): DistanceBar[] {
  const longest = Math.max(...bands.map((b) => b.interval));
  return distanceRows(bands).map((r, i) => ({
    band: bands[i].hi === OPEN_BAND ? `${bands[i].lo}°+` : r.band,
    kill: r.kill,
    short: r.short,
    share: longest > 0 ? bands[i].interval / longest : 0,
  }));
}

/** The checks, those to look at first. */
export function sortedIssues(issues: Issue[]): Issue[] {
  return [...issues].sort(
    (a, b) => Number(b.flag === 'attention') - Number(a.flag === 'attention'),
  );
}

/** A row of the by-distance table, as shown. */
export interface DistanceRow {
  band: string;
  flicks: number;
  kill: string;
  reaction: string;
  short: string;
  past: string;
  still: string;
}

/** The widest band ends at 90°: it reads "60° and over". */
const OPEN_BAND = 90;

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
  toward: string;
  flicks: number;
  kill: string;
  distance: string;
  beyond: string;
  short: string;
  past: string;
}

export function directionRows(bands: DirectionBand[]): DirectionRow[] {
  return bands.map((b) => ({
    toward: `${DIRECTION_ARROWS[b.name]} ${b.name}`,
    flicks: b.n,
    kill: formatMs(b.interval),
    distance: `${b.distance.toFixed(1)}°`,
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
function plus(n: number): string {
  return `+${Math.abs(n) < 9.95 ? n.toFixed(1) : n.toFixed(0)}`;
}

/**
 * The what-if table: the extra kills each change would give, and the extra score where it is known, under Pace, Flicks
 * and Micros, each group biggest first. A group with no lines is left out, and so is the table on reports that lack
 * the lines (older cores).
 */
export function clickWhatIf(w: ClickWhatIf[] | undefined): WhatIfTable {
  const lines = w ?? [];
  const score = lines.some((l) => l.score !== null);
  const groups = WHAT_IF_GROUPS.map(([group, name]) => ({
    name,
    lines: lines
      .filter((l) => l.group === group)
      .sort((a, b) => b.kills - a.kills)
      .map((l) => ({
        what: l.what,
        gains: score
          ? [`${plus(l.kills)} kills`, l.score === null ? '' : `${plus(l.score)} score`]
          : [`${plus(l.kills)} kills`],
        how: l.how,
      })),
  })).filter((g) => g.lines.length);
  return { columns: score ? ['Kills', 'Score'] : ['Kills'], groups };
}
