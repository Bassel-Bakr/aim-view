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
  for (const trackFrame of frames) {
    if (!trackFrame.s) continue;
    trackFrame.t.forEach(([id, x, y], index) => {
      if (Math.hypot(x, y) < near || trackFrame.s?.[index] === undefined) return;
      let trackScores = seen.get(id);
      if (!trackScores) seen.set(id, (trackScores = []));
      trackScores.push(trackFrame.s[index]);
    });
  }
  const scores = new Map<number, number>();
  const counts = new Map<number, number>();
  for (const [id, trackScores] of seen) {
    if (trackScores.length < LEAST_FRAMES) continue;
    trackScores.sort((a, b) => a - b);
    scores.set(
      id,
      trackScores[
        Math.min(trackScores.length - 1, Math.floor(PERCENTILE * (trackScores.length - 1) + 0.5))
      ],
    );
    counts.set(id, trackScores.length);
  }
  const order = [...scores.keys()].sort((a, b) => (scores.get(a) ?? 0) - (scores.get(b) ?? 0));
  const total = order.reduce((sum, id) => sum + (counts.get(id) ?? 0), 0);
  let framesSoFar = 0;
  let level: number | null = null;
  for (const id of order) {
    framesSoFar += counts.get(id) ?? 0;
    if (framesSoFar >= PERCENTILE * total) {
      level = scores.get(id) ?? null;
      break;
    }
  }
  return { scores, frames: counts, level };
}

/** The tracks scoring under the cut (the level less the offset). */
export function tracksUnder(scored: FaintScores, cut: number): Set<number> {
  return new Set([...scored.scores].filter(([, score]) => score < cut).map(([id]) => id));
}

/** The frame without the tracks given: each target's place, area, box and score alike. */
function frameWithout(frame: TrackFrame, gone: ReadonlySet<number>): TrackFrame {
  const keep = frame.t.map(([id]) => !gone.has(id));
  if (keep.every(Boolean)) return frame;
  const kept = (_target: unknown, index: number) => keep[index];
  const pick = <T>(values: readonly T[] | undefined) => values?.filter(kept);
  return {
    ...frame,
    // eslint-disable-next-line id-length -- the core names TrackFrame's fields (generated/track-frame.ts)
    t: frame.t.filter(kept),
    a: frame.a.filter(kept),
    wh: pick(frame.wh),
    // eslint-disable-next-line id-length -- the core names TrackFrame's fields (generated/track-frame.ts)
    s: pick(frame.s),
  };
}

/** The tracks without those the cut-off leaves out (python/review.py: `without_faint`). */
export function tracksWithout(tracks: Tracks, gone: ReadonlySet<number>): Tracks {
  return gone.size
    ? { ...tracks, frames: tracks.frames.map((frame) => frameWithout(frame, gone)) }
    : tracks;
}
