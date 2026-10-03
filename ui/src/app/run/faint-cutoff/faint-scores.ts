import { TrackFrame, Tracks } from '../../api';

/**
 * The faint-target cut-off's scores (python/review.py: `faint_scores`): each track's score, the 90th percentile of the
 * detector's scores for it away from the crosshair, how many frames gave one, in the order the tracks first scored;
 * and the recording's level, the 90th percentile of the scores weighted by frames (null without scores).
 */
export interface FaintScores {
  scores: ReadonlyMap<number, number>;
  frames: ReadonlyMap<number, number>;
  level: number | null;
}

/** The fewest frames a track needs for a score. */
const LEAST_FRAMES = 3;
const PERCENTILE = 0.9;

/**
 * Each track's score from the frames where it lies `near` degrees or more from the crosshair (a target under the
 * crosshair scores low; a tracking run counts them all: near 0), for tracks with 3 or more such frames.
 */
export function faintScores(frames: readonly TrackFrame[], near: number): FaintScores {
  const seen = new Map<number, number[]>();
  for (const f of frames) {
    if (!f.s) continue;
    f.t.forEach(([id, x, y], k) => {
      if (Math.hypot(x, y) < near || f.s?.[k] === undefined) return;
      let v = seen.get(id);
      if (!v) seen.set(id, (v = []));
      v.push(f.s[k]);
    });
  }
  const scores = new Map<number, number>();
  const counts = new Map<number, number>();
  for (const [id, v] of seen) {
    if (v.length < LEAST_FRAMES) continue;
    v.sort((a, b) => a - b);
    scores.set(id, v[Math.min(v.length - 1, Math.floor(PERCENTILE * (v.length - 1) + 0.5))]);
    counts.set(id, v.length);
  }
  const order = [...scores.keys()].sort((a, b) => (scores.get(a) ?? 0) - (scores.get(b) ?? 0));
  const total = order.reduce((n, id) => n + (counts.get(id) ?? 0), 0);
  let acc = 0;
  let level: number | null = null;
  for (const id of order) {
    acc += counts.get(id) ?? 0;
    if (acc >= PERCENTILE * total) {
      level = scores.get(id) ?? null;
      break;
    }
  }
  return { scores, frames: counts, level };
}

/** The tracks scoring under the cut (the level less the offset). */
export function tracksUnder(sc: FaintScores, cut: number): Set<number> {
  return new Set([...sc.scores].filter(([, v]) => v < cut).map(([id]) => id));
}

/** The frame without the tracks given: each target's place, area, box and score alike. */
function frameWithout(f: TrackFrame, gone: ReadonlySet<number>): TrackFrame {
  const keep = f.t.map(([id]) => !gone.has(id));
  if (keep.every(Boolean)) return f;
  const pick = <T>(v: readonly T[] | undefined) => v?.filter((_, k) => keep[k]);
  return { ...f, t: f.t.filter((_, k) => keep[k]), a: pick(f.a), wh: pick(f.wh), s: pick(f.s) };
}

/** The tracks without those the cut-off leaves out (python/review.py: `without_faint`). */
export function tracksWithout(tracks: Tracks, gone: ReadonlySet<number>): Tracks {
  return gone.size
    ? { ...tracks, frames: tracks.frames.map((f) => frameWithout(f, gone)) }
    : tracks;
}
