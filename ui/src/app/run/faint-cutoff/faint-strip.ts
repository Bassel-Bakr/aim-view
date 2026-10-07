/**
 * The cut-off panel's strip: every scored track as a dot at its score, and the cut as a line. In:
 * the tracks' faint scores (faint-scores.ts) and the cut. Out: the SVG the cut-off panel draws, in
 * its view box's units.
 */

import { FaintScores } from './faint-scores';

/** A track in the strip: its place and size, whether the cut leaves it out, and its words. */
export interface StripDot {
  /** The track's id. */
  id: number;
  /** The dot's center along the strip, in view box units (its score). */
  x: number;
  /** The dot's center down the strip, in view box units (spread by its id). */
  y: number;
  /** The dot's radius in view box units, larger for a track seen in more frames. */
  radius: number;
  /** The cut leaves the track out. */
  out: boolean;
  /** The dot's tooltip: the track's score and frames. */
  title: string;
}

/** A score mark under the strip. */
export interface StripTick {
  /** The mark's place along the strip, in view box units. */
  x: number;
  /** The score it marks, to one decimal. */
  text: string;
}

/** Every track at its score, the score marks, and the cut's place. */
export interface Strip {
  /** A dot for each scored track. */
  dots: StripDot[];
  /** The score marks. */
  ticks: StripTick[];
  /** The cut's place along the strip, in view box units. */
  cutX: number;
}

/** The strip's width, in its SVG's view box units. */
export const STRIP_WIDTH = 600;
/** The strip's height, in its SVG's view box units. */
export const STRIP_HEIGHT = 52;
/** The lowest score the strip shows; lower ones sit at its left end. */
const LOW = 0.2;
/** The highest score the strip shows; higher ones sit at its right end. */
const HIGH = 1.0;
/** The space at the strip's ends and above its dots, in view box units. */
const PAD = 8;
/** The scores marked under the strip. */
const TICKS = [0.2, 0.4, 0.6, 0.8, 1.0];
/**
 * The dots spread over this many units of height, by their id, so near scores do not hide each
 * other.
 */
const SPREAD = 26;
/** Knuth's multiplicative hash, which turns a track id into its place in the spread. */
const SPREAD_HASH = 2654435761;
/** The largest dot's radius, in view box units. */
const MAX_RADIUS = 6;

/**
 * A score's place along the strip, in view box units; scores outside LOW to HIGH sit at its ends.
 */
function scoreX(score: number): number {
  const clamped = Math.min(HIGH, Math.max(LOW, score));
  return PAD + ((STRIP_WIDTH - 2 * PAD) * (clamped - LOW)) / (HIGH - LOW);
}

/**
 * Every track's score as a dot along the strip, the cut as a line: where the seams and the targets
 * sit. A dot's size follows how long the track was seen.
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
