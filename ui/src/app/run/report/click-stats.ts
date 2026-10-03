import { ClickSummary, DirectionBand, DistanceBand, Flick, Issue } from '../../api';
import {
  arrow,
  DIRECTION_ARROWS,
  formatCount,
  formatDegrees,
  formatEnded,
  formatMs,
  formatPercent,
  formatSpeed,
} from '../../format';
import { median } from '../median';

/** A number card: its value, what it is, and a line under it. */
export interface Stat {
  label: string;
  value: string;
  detail: string;
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
 * The whole run's cards: the score and kills, then the times and speeds, then the path; sixteen, as many as a kill
 * has, so the rows stay put when a kill is picked.
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
    { label: 'Median kill', value: formatMs(s.median_interval), detail: '' },
    { label: 'Still before the click', value: formatMs(s.still), detail: '' },
    { label: 'Peak speed', value: formatSpeed(s.peak), detail: '' },
    { label: 'Game FPS', value: s.fps_avg ? String(Math.round(s.fps_avg)) : '–', detail: '' },
    {
      label: 'Fastest next target',
      value: paths ? formatPercent(paths.share) : LOADING,
      detail: 'of the picks with a choice',
    },
    { label: 'Path cost in all', value: paths ? formatMs(paths.total) : LOADING, detail: '' },
    {
      label: `More ${s.shots == null ? 'kills' : 'shots'} with the best path`,
      value: paths ? `≈ ${paths.extra.toFixed(1)}` : LOADING,
      detail: 'at your pace',
    },
    { label: 'Accuracy', value: formatPercent(s.accuracy), detail: '' },
    { label: 'Reaction', value: formatMs(s.react), detail: 'median' },
    { label: 'Main flick', value: formatMs(s.flick), detail: 'median' },
    { label: 'Click speed', value: formatSpeed(s.click_speed), detail: 'median' },
    { label: 'Off center at the click', value: formatDegrees(s.click_off), detail: 'median' },
    {
      label: 'Kills a minute',
      value: pace == null ? '–' : pace.toFixed(1),
      detail: 'first flick to last kill',
    },
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
  const corrections = runMedian(flicks, (f) => f.corr);
  return [
    { label: 'Distance', value: `${m.D0.toFixed(1)}° ${arrow(m.dir)}`, detail: '' },
    { label: 'Kill time', value: formatMs(m.total), detail: run(formatMs(s.median_interval)) },
    { label: 'Reaction', value: formatMs(m.react), detail: run(formatMs(s.react)) },
    { label: 'Main flick', value: formatMs(m.flick), detail: run(formatMs(s.flick)) },
    { label: 'Main flick ended', value: formatEnded(m.end_left, s.radius), detail: '' },
    { label: 'Still before the click', value: formatMs(m.still), detail: run(formatMs(s.still)) },
    { label: 'Peak speed', value: formatSpeed(m.peak), detail: run(formatSpeed(s.peak)) },
    {
      label: 'Click speed',
      value: formatSpeed(m.click_speed),
      detail: run(formatSpeed(s.click_speed)),
    },
    { label: 'Shots', value: formatCount(m.shots), detail: '' },
    { label: 'Path cost', value: pathCost, detail: 'against the fastest pick' },
    {
      label: 'Onto the target',
      value: formatMs(m.arrive),
      detail: run(formatMs(runMedian(flicks, (f) => f.arrive))),
    },
    {
      label: 'Settle',
      value: formatMs(m.settle),
      detail: run(formatMs(runMedian(flicks, (f) => f.settle))),
    },
    {
      label: 'Off center at the click',
      value: formatDegrees(m.click_off),
      detail: run(formatDegrees(s.click_off)),
    },
    {
      label: 'Corrections',
      value: formatCount(m.corr),
      detail: corrections == null ? '' : run(String(corrections)),
    },
    {
      label: 'Went past by',
      value: formatDegrees(Math.max(0, m.past - s.radius)),
      detail: 'past the far edge',
    },
    {
      label: 'New target',
      value: m.spawned ? 'yes' : 'no',
      detail: 'it appeared after the flick began',
    },
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
