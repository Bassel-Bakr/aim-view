import { ClickReport, Flick, Issue, TrackPoint, Tracks } from '../../api';
import { formatMs, formatPercent } from '../../format';
import { fitFitts, OrderSolver } from './order-solver';

/** How a kill's pick of the next target went. cost: seconds lost against the fastest pick. */
export interface KillPick {
  cost: number;
  best: boolean;
  choices: number;
  /** Its target appeared too late to count as a choice (a reaction of its own). */
  spawned: boolean;
}

/**
 * Every kill's pick, which track each kill was (killOf), when each target first showed (firstSeen), the time lost in
 * all, the share of fastest picks, and the run's frames (from the first flick's start, or the user's mark, to the last
 * kill, or the user's end).
 */
export interface PathAnalysis {
  solver: OrderSolver;
  picks: Map<number, KillPick>;
  killOf: Map<number, Flick>;
  firstSeen: Map<number, number>;
  total: number;
  share: number | null;
  firstStart: number;
  lastKill: number;
}

/** A target newer than this when the flick began needs a reaction of its own, which the model does not price. */
export const NEW_MS = 150;
/** The pick is read this many frames after the flick starts, when the target just killed has gone. */
const PICK_FRAME = 3;
/** A kill's path ends on its target's track within this many degrees. */
const SAME_PLACE = 0.05;
/** Without a stats file, the run is taken to start two median kills before the first kill. */
const LEAD_IN_KILLS = 2;
const DEFAULT_KILL = 0.5;

/** The last frame a target may first show on and still count as a choice for the flick. */
export function newCut(flick: Flick, fps: number): number {
  return flick.start_frame + (flick.react ?? 0) * fps - (NEW_MS / 1000) * fps;
}

/** The run's first and last frames: the first flick's start (or the user's mark) and the last kill (or the user's end). */
type RunFrames = [firstStart: number, lastKill: number];

function runFrames(report: ClickReport): RunFrames {
  let lastKill = Math.max(...report.flicks.map((flick) => flick.kill_frame));
  const first = report.flicks.reduce((a, b) => (b.kill_frame < a.kill_frame ? b : a));
  const leadIn = Math.round(
    LEAD_IN_KILLS * (report.summary.median_interval || DEFAULT_KILL) * report.fps,
  );
  let firstStart =
    report.summary.info.source === 'stats'
      ? first.start_frame
      : Math.max(first.start_frame, first.kill_frame - leadIn);
  if (report.run?.start != null) firstStart = Math.round(report.run.start * report.fps);
  if (report.run?.end != null) lastKill = Math.round(report.run.end * report.fps);
  return [firstStart, lastKill];
}

/** When each target (by its track's id) first showed: from the report where it says, else from the tracks. */
function firstSeenFrames(report: ClickReport, tracks: Tracks): Map<number, number> {
  const firstSeen = new Map<number, number>();
  if (report.appeared) {
    for (const [id, frame] of Object.entries(report.appeared)) firstSeen.set(Number(id), frame);
  } else {
    tracks.frames.forEach((trackFrame, i) =>
      trackFrame.t.forEach((target) => firstSeen.has(target[0]) || firstSeen.set(target[0], i)),
    );
  }
  return firstSeen;
}

/** Which kill each track was (by the track's id), and each kill's track (by its kill number). */
interface KillTracks {
  killOf: Map<number, Flick>;
  trackOf: Map<number, number>;
}

/** A kill's track is the one its path ends on. */
function killTracks(report: ClickReport, tracks: Tracks): KillTracks {
  const killOf = new Map<number, Flick>();
  const trackOf = new Map<number, number>();
  for (const flick of report.flicks) {
    const path = report.paths[String(flick.kill_number)];
    const last = path?.[path.length - 1];
    const hit =
      last &&
      tracks.frames[last[0]]?.t.find(
        (target) =>
          Math.abs(target[1] - last[1]) < SAME_PLACE && Math.abs(target[2] - last[2]) < SAME_PLACE,
      );
    if (hit) {
      killOf.set(hit[0], flick);
      trackOf.set(flick.kill_number, hit[0]);
    }
  }
  return { killOf, trackOf };
}

/** What every kill's pick is measured against: the run, its tracks, when each target showed, and the solver. */
interface PickInputs {
  report: ClickReport;
  tracks: Tracks;
  firstSeen: Map<number, number>;
  solver: OrderSolver;
}

/** How a kill's pick of its target (track id) went; null where the tracks cannot tell. */
function killPick(flick: Flick, id: number, inputs: PickInputs): KillPick | null {
  const { report, tracks, firstSeen, solver } = inputs;
  const cut = newCut(flick, report.fps);
  // a target with no known first frame is never taken for a new one, and never offered as a choice
  if ((firstSeen.get(id) ?? -Infinity) > cut) {
    return { cost: 0, best: false, choices: 0, spawned: true };
  }
  // the target just killed can linger a frame under the crosshair, and new targets were not options
  const choices = (tracks.frames[flick.start_frame + PICK_FRAME]?.t ?? []).filter(
    (target: TrackPoint) =>
      target[0] === id ||
      (Math.hypot(target[1], target[2]) > report.summary.radius &&
        (firstSeen.get(target[0]) ?? Infinity) <= cut),
  );
  if (!choices.some((target) => target[0] === id)) return null;
  if (choices.length === 1) return { cost: 0, best: true, choices: 1, spawned: false };
  const solution = solver.solve(choices);
  const j = solution ? solution.targets.findIndex((target) => target[0] === id) : -1;
  if (!solution || j < 0) return null;
  return {
    cost: solver.fitts.b * (solution.from[j] - solution.from[solution.bestFirst]),
    best: j === solution.bestFirst,
    choices: choices.length,
    spawned: false,
  };
}

export function analysePaths(report: ClickReport, tracks: Tracks): PathAnalysis | null {
  if (!report.flicks.length) return null;
  const solver = new OrderSolver(fitFitts(report.flicks, report.summary.radius));
  const firstSeen = firstSeenFrames(report, tracks);
  const { killOf, trackOf } = killTracks(report, tracks);
  const inputs: PickInputs = { report, tracks, firstSeen, solver };
  const picks = new Map<number, KillPick>();
  for (const flick of report.flicks) {
    const id = trackOf.get(flick.kill_number);
    if (id == null) continue;
    const pick = killPick(flick, id, inputs);
    if (pick) picks.set(flick.kill_number, pick);
  }
  const all = [...picks.values()];
  const withChoice = all.filter((pick) => pick.choices > 1);
  const [firstStart, lastKill] = runFrames(report);
  return {
    solver,
    picks,
    killOf,
    firstSeen,
    total: all.reduce((sum, pick) => sum + pick.cost, 0),
    share: withChoice.length
      ? withChoice.filter((pick) => pick.best).length / withChoice.length
      : null,
    firstStart,
    lastKill,
  };
}

/** A kill's pick in words: fastest, only one, spawn (a new target), or the time it cost. "…" while the tracks load. */
export function pickText(analysis: PathAnalysis | null, killNumber: number): string {
  if (!analysis) return '…';
  const pick = analysis.picks.get(killNumber);
  if (!pick) return '–';
  if (pick.spawned) return 'spawn';
  if (pick.choices === 1) return 'only one';
  return pick.best ? 'fastest' : `+${Math.round(1000 * pick.cost)} ms`;
}

/**
 * Time lost to picks, as shots: at the run's pace (shots from the first flick to the last kill), the time the best picks
 * would have saved, in shots (kills without a stats file). An estimate: it assumes the pace holds.
 */
export function extraShots(analysis: PathAnalysis, report: ClickReport, seconds: number): number {
  const start = Math.min(...report.flicks.map((flick) => flick.start_frame));
  const span = (analysis.lastKill - start) / report.fps;
  const count = report.summary.shots ?? report.summary.kills;
  return span > 0 && count ? (seconds * count) / span : 0;
}

/** A costly pick, linked from the Pathing check. */
export interface CostlyPick {
  flick: Flick;
  cost: string;
}

/**
 * The Pathing check, the page's own (it has no issue number, which the core's checks have), with its costliest picks.
 */
export interface Pathing {
  issue: Omit<Issue, 'issue'>;
  costliest: CostlyPick[];
}

/** Picks that cost this share of the median TTK or more are flagged. */
const FLAG_SHARE = 0.05;
const COSTLIEST = 3;

export function pathing(analysis: PathAnalysis | null, report: ClickReport): Pathing | null {
  if (!analysis) return null;
  const picks = [...analysis.picks.entries()].filter(([, pick]) => pick.choices > 1);
  if (!picks.length) return null;
  const lost = picks.reduce((sum, [, pick]) => sum + pick.cost, 0);
  const per = lost / picks.length;
  const median = report.summary.median_interval;
  const share = median ? per / median : 0;
  const unit = report.summary.shots == null ? 'kills' : 'shots';
  const byKillNumber = new Map(report.flicks.map((flick) => [flick.kill_number, flick]));
  const costliest = picks
    .filter(([, pick]) => !pick.best)
    .sort((a, b) => b[1].cost - a[1].cost)
    .slice(0, COSTLIEST)
    .map(([killNumber, pick]) => ({
      flick: byKillNumber.get(killNumber) as Flick,
      cost: `+${formatMs(pick.cost)}`,
    }));
  return {
    issue: {
      title: 'Pathing',
      flag: share >= FLAG_SHARE ? 'attention' : 'fine',
      value:
        `The fastest next target in ${formatPercent(analysis.share)} of ${picks.length} picks; the others cost about ` +
        `${formatMs(lost)} in all, ${formatMs(per)} a kill (${formatPercent(share)} of the median TTK). With the best ` +
        `picks, about ${extraShots(analysis, report, lost).toFixed(1)} more ${unit} at your pace`,
      why:
        "Predicted from Fitts' law fitted to this run, for the targets on screen at each pick; targets that appeared " +
        `less than ${NEW_MS} ms before you started moving are left out, since reacting to them costs time of its own. ` +
        '5% of the median TTK or more is flagged.',
    },
    costliest,
  };
}
