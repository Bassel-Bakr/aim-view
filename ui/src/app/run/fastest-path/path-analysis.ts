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

/** The last frame a target may first show on and still count as a choice for flick m. */
export function newCut(m: Flick, fps: number): number {
  return m.start_frame + (m.react ?? 0) * fps - (NEW_MS / 1000) * fps;
}

/** The run's first and last frames: the first flick's start (or the user's mark) and the last kill (or the user's end). */
type RunFrames = [firstStart: number, lastKill: number];

function runFrames(r: ClickReport): RunFrames {
  let lastKill = Math.max(...r.flicks.map((m) => m.kill_frame));
  const first = r.flicks.reduce((a, b) => (b.kill_frame < a.kill_frame ? b : a));
  const leadIn = Math.round(LEAD_IN_KILLS * (r.summary.median_interval || DEFAULT_KILL) * r.fps);
  let firstStart =
    r.summary.info.source === 'stats'
      ? first.start_frame
      : Math.max(first.start_frame, first.kill_frame - leadIn);
  if (r.run?.start != null) firstStart = Math.round(r.run.start * r.fps);
  if (r.run?.end != null) lastKill = Math.round(r.run.end * r.fps);
  return [firstStart, lastKill];
}

export function analysePaths(r: ClickReport, tracks: Tracks): PathAnalysis | null {
  if (!r.flicks.length) return null;
  const solver = new OrderSolver(fitFitts(r.flicks, r.summary.radius));
  const firstSeen = new Map<number, number>();
  if (r.appeared) for (const [k, v] of Object.entries(r.appeared)) firstSeen.set(Number(k), v);
  else
    tracks.frames.forEach((f, i) =>
      f.t.forEach((t) => firstSeen.has(t[0]) || firstSeen.set(t[0], i)),
    );
  const killOf = new Map<number, Flick>();
  const trackOf = new Map<number, number>();
  for (const m of r.flicks) {
    const path = r.paths[String(m.n)];
    const p = path?.[path.length - 1];
    const hit =
      p &&
      tracks.frames[p[0]]?.t.find(
        (t) => Math.abs(t[1] - p[1]) < SAME_PLACE && Math.abs(t[2] - p[2]) < SAME_PLACE,
      );
    if (hit) {
      killOf.set(hit[0], m);
      trackOf.set(m.n, hit[0]);
    }
  }
  const picks = new Map<number, KillPick>();
  for (const m of r.flicks) {
    const id = trackOf.get(m.n);
    if (id == null) continue;
    const cut = newCut(m, r.fps);
    // a target with no known first frame is never taken for a new one, and never offered as a choice
    if ((firstSeen.get(id) ?? -Infinity) > cut) {
      picks.set(m.n, { cost: 0, best: false, choices: 0, spawned: true });
      continue;
    }
    // the target just killed can linger a frame under the crosshair, and new targets were not options
    const ts = (tracks.frames[m.start_frame + PICK_FRAME]?.t ?? []).filter(
      (t: TrackPoint) =>
        t[0] === id ||
        (Math.hypot(t[1], t[2]) > r.summary.radius && (firstSeen.get(t[0]) ?? Infinity) <= cut),
    );
    if (!ts.some((t) => t[0] === id)) continue;
    if (ts.length === 1) {
      picks.set(m.n, { cost: 0, best: true, choices: 1, spawned: false });
      continue;
    }
    const sol = solver.solve(ts);
    const j = sol ? sol.pts.findIndex((p) => p[0] === id) : -1;
    if (!sol || j < 0) continue;
    picks.set(m.n, {
      cost: solver.fitts.b * (sol.from[j] - sol.from[sol.j0]),
      best: j === sol.j0,
      choices: ts.length,
      spawned: false,
    });
  }
  const all = [...picks.values()];
  const withChoice = all.filter((x) => x.choices > 1);
  const [firstStart, lastKill] = runFrames(r);
  return {
    solver,
    picks,
    killOf,
    firstSeen,
    total: all.reduce((s, x) => s + x.cost, 0),
    share: withChoice.length ? withChoice.filter((x) => x.best).length / withChoice.length : null,
    firstStart,
    lastKill,
  };
}

/** A kill's pick in words: fastest, only one, new target, or the time it cost. "…" while the tracks load. */
export function pickText(a: PathAnalysis | null, n: number): string {
  if (!a) return '…';
  const o = a.picks.get(n);
  if (!o) return '–';
  if (o.spawned) return 'new target';
  if (o.choices === 1) return 'only one';
  return o.best ? 'fastest' : `+${Math.round(1000 * o.cost)} ms`;
}

/**
 * Time lost to picks, as shots: at the run's pace (shots from the first flick to the last kill), the time the best picks
 * would have saved, in shots (kills without a stats file). An estimate: it assumes the pace holds.
 */
export function extraShots(a: PathAnalysis, r: ClickReport, seconds: number): number {
  const start = Math.min(...r.flicks.map((m) => m.start_frame));
  const span = (a.lastKill - start) / r.fps;
  const count = r.summary.shots ?? r.summary.kills;
  return span > 0 && count ? (seconds * count) / span : 0;
}

/** A costly pick, linked from the Pathing check. */
export interface CostlyPick {
  flick: Flick;
  cost: string;
}

/** The Pathing check, with its costliest picks. */
export interface Pathing {
  issue: Issue;
  costliest: CostlyPick[];
}

/** Picks that cost this share of the median kill or more are flagged. */
const FLAG_SHARE = 0.05;
const COSTLIEST = 3;

export function pathing(a: PathAnalysis | null, r: ClickReport): Pathing | null {
  if (!a) return null;
  const picks = [...a.picks.entries()].filter(([, o]) => o.choices > 1);
  if (!picks.length) return null;
  const lost = picks.reduce((t, [, o]) => t + o.cost, 0);
  const per = lost / picks.length;
  const median = r.summary.median_interval;
  const share = median ? per / median : 0;
  const unit = r.summary.shots == null ? 'kills' : 'shots';
  const byN = new Map(r.flicks.map((m) => [m.n, m]));
  const costliest = picks
    .filter(([, o]) => !o.best)
    .sort((x, y) => y[1].cost - x[1].cost)
    .slice(0, COSTLIEST)
    .map(([n, o]) => ({ flick: byN.get(n) as Flick, cost: `+${formatMs(o.cost)}` }));
  return {
    issue: {
      title: 'Pathing',
      flag: share >= FLAG_SHARE ? 'attention' : 'fine',
      value:
        `The fastest next target in ${formatPercent(a.share)} of ${picks.length} picks; the others cost about ` +
        `${formatMs(lost)} in all, ${formatMs(per)} a kill (${formatPercent(share)} of the median kill). With the best ` +
        `picks, about ${extraShots(a, r, lost).toFixed(1)} more ${unit} at your pace`,
      why:
        "Predicted from Fitts' law fitted to this run, for the targets on screen at each pick; targets that appeared " +
        `less than ${NEW_MS} ms before you started moving are left out, since reacting to them costs time of its own. ` +
        '5% of the median kill or more is flagged.',
    },
    costliest,
  };
}
