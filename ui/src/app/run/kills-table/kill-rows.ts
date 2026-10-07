/**
 * A clicking run's kills as the kills table shows them (kills-table.ts): what each cell says, the
 * number each column sorts by, and the groups a kill falls in. Built from the report (the core's
 * measures) and the fastest-path analysis. Out: the flick list's kills table.
 */

import { ClickReport, Flick } from '../../api';
import { arrow, formatCount, formatDegrees, formatEnded } from '../../format';
import { PathAnalysis, pickText } from '../fastest-path/path-analysis';
import { micro, microSplit } from '../report/budget';

/** A kill as the table shows it. */
export interface FlickRow {
  /** The kill's measures, which the columns sort by. */
  flick: Flick;
  /** The kill's number in the run, from 1. */
  killNumber: number;
  /** The distance and the way it was: "12.3° ↑". */
  distance: string;
  /** The time to kill, in whole milliseconds. */
  ttk: string;
  /** How the flick landed: on target, or the degrees short of it or past it (formatEnded). */
  landed: string;
  /** How long the crosshair was still on the target before the click, in whole milliseconds. */
  confirmation: string;
  /** The flick's top speed, in whole degrees a second. */
  flickSpeed: string;
  /** The crosshair's speed at the click, in whole degrees a second. */
  onTheMove: string;
  /** The shots at the target. */
  shots: string;
  /** The first shot missed (more than one shot). */
  missed: boolean;
  /** What picking this target cost against the fastest pick. */
  pathing: string;
  /**
   * The kill's steps in one cell, to keep the table narrow: reaction, flick and micro,
   * "67 · 183 · 125 ms".
   */
  steps: string;
  /** The micro's two parts, on hover: "120 ms onto the target, 80 ms settling". */
  microSplit: string;
  /** How far the click was from the target's center (formatDegrees). */
  offCenter: string;
  /** How many separate movements came after the flick. */
  micros: string;
  /** "yes" when the target appeared after the flick began, else "no". */
  spawn: string;
  /** The seconds the pick cost against the fastest one; null without the analysis. */
  pickCost: number | null;
  /** The groups the kill falls in, by what the table can group on. */
  groups: Record<KillGrouping, string>;
}

/** What the table can group the kills by. */
export type KillGrouping = 'landed' | 'direction' | 'distance' | 'firstShot' | 'spawn';

/** A grouping as the table offers it. */
export interface KillGroupingChoice {
  /** The grouping, or null for none. */
  key: KillGrouping | null;
  /** Its name in the Group by menu. */
  label: string;
}

/** The groupings the kills table offers, in the Group by menu's order. */
export const GROUPINGS: readonly KillGroupingChoice[] = [
  { key: null, label: 'Nothing' },
  { key: 'landed', label: 'How the flick landed' },
  { key: 'direction', label: 'Direction' },
  { key: 'distance', label: 'Distance' },
  { key: 'firstShot', label: 'First shot' },
  { key: 'spawn', label: 'Spawn' },
];

/**
 * A number rounded after scaling (seconds by 1000: milliseconds), without its unit, which the
 * column's header names (the table stays narrow enough to fit beside the player); a dash when not
 * measured.
 */
function whole(value: number | null | undefined, scale = 1): string {
  return value == null ? '–' : String(Math.round(scale * value));
}

/**
 * How a flick landed, in a word: on the target, short of it, or past it (formatEnded's three
 * cases). The radius is the target's, in degrees.
 */
function landedKind(flick: Flick, radiusDeg: number): string {
  if (flick.end_left > radiusDeg) return 'Underflick';
  if (flick.end_left < -radiusDeg) return 'Overflick';
  return 'On target';
}

/**
 * The report's distance band a flick's start falls in ("5–10°"), as its By distance table bands
 * them.
 */
function distanceBand(flick: Flick, report: ClickReport): string {
  const band = report.summary.by_distance.find(
    (group) => flick.D0 >= group.lo && flick.D0 < group.hi,
  );
  return band ? `${band.lo}–${band.hi}°` : 'Other';
}

/** The group a kill falls in for each grouping, as its group's heading. */
function groupsOf(flick: Flick, report: ClickReport): Record<KillGrouping, string> {
  return {
    landed: landedKind(flick, report.summary.radius),
    direction: arrow(flick.direction_deg),
    distance: distanceBand(flick, report),
    firstShot: (flick.shots ?? 0) > 1 ? 'Missed the first shot' : 'Hit with the first shot',
    spawn: flick.spawned ? 'Spawned during the flick' : 'On screen at the start',
  };
}

/**
 * Every kill of the run as a row, in the report's order; the Pathing cells say "…" until the
 * fastest-path analysis is in.
 */
export function flickRows(report: ClickReport, paths: PathAnalysis | null): FlickRow[] {
  return report.flicks.map((flick) => ({
    flick: flick,
    killNumber: flick.kill_number,
    distance: `${flick.D0.toFixed(1)}° ${arrow(flick.direction_deg)}`,
    ttk: whole(flick.total, 1000),
    landed: formatEnded(flick.end_left, report.summary.radius),
    confirmation: whole(flick.still, 1000),
    flickSpeed: whole(flick.peak),
    onTheMove: whole(flick.click_speed),
    shots: formatCount(flick.shots),
    missed: (flick.shots ?? 0) > 1,
    pathing: pickText(paths, flick.kill_number),
    steps: [flick.react, flick.flick, micro(flick)]
      .map((seconds) => whole(seconds, 1000))
      .join(' · '),
    microSplit: flick.parts ? microSplit(flick.parts) : '',
    offCenter: formatDegrees(flick.click_off),
    micros: formatCount(flick.corrections),
    spawn: flick.spawned ? 'yes' : 'no',
    pickCost: paths?.picks.get(flick.kill_number)?.cost ?? null,
    groups: groupsOf(flick, report),
  }));
}
