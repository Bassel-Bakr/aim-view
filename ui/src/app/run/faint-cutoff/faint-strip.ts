import { FaintScores } from './faint-scores';

/** A track in the strip: its place and size, whether the cut leaves it out, and its words. */
export interface StripDot {
  id: number;
  x: number;
  y: number;
  radius: number;
  out: boolean;
  title: string;
}

/** A score mark under the strip. */
export interface StripTick {
  x: number;
  text: string;
}

/** Every track at its score, the score marks, and the cut's place. */
export interface Strip {
  dots: StripDot[];
  ticks: StripTick[];
  cutX: number;
}

/** The strip's size (its SVG's view box) and the scores it spans. */
export const STRIP_WIDTH = 600;
export const STRIP_HEIGHT = 52;
const LOW = 0.2;
const HIGH = 1.0;
const PAD = 8;
const TICKS = [0.2, 0.4, 0.6, 0.8, 1.0];
/** The dots spread over this many units of height, by their id, so near scores do not hide each other. */
const SPREAD = 26;
const SPREAD_HASH = 2654435761;
const MAX_RADIUS = 6;

/** A score's place along the strip. */
function scoreX(score: number): number {
  const clamped = Math.min(HIGH, Math.max(LOW, score));
  return PAD + ((STRIP_WIDTH - 2 * PAD) * (clamped - LOW)) / (HIGH - LOW);
}

/**
 * Every track's score as a dot along the strip, the cut as a line: where the seams and the targets sit. A dot's size
 * follows how long the track was seen.
 */
export function faintStrip(scored: FaintScores, cut: number): Strip {
  const dots = [...scored.scores].map(([id, score]): StripDot => {
    const frameCount = scored.frames.get(id) ?? 0;
    return {
      id,
      x: scoreX(score),
      y: PAD + (((id * SPREAD_HASH) % 1000) / 1000) * SPREAD,
      radius: Math.min(MAX_RADIUS, 1.5 + Math.sqrt(frameCount) / 4),
      out: score < cut,
      title: `Track ${id}: score ${score.toFixed(2)}, seen in ${frameCount} frames away from the crosshair. Click to show it`,
    };
  });
  return {
    dots,
    ticks: TICKS.map((tick) => ({ x: scoreX(tick), text: tick.toFixed(1) })),
    cutX: scoreX(cut),
  };
}
