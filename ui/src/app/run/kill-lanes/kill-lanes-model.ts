/**
 * The kill lanes' shapes, for their SVG: the line between the lanes, and for each kill its mark above and its time as a
 * bar below, at the kill's moment.
 *
 * In: the clicking report's kills (their frames and TTKs), the run's length, the picked kill, and the lanes' sizes from
 * the tokens (tokens.ts, from themes/timeline.scss; the same in both themes). Out: the shapes kill-lanes.html draws;
 * their colors are the stylesheet's (kill-lanes.scss), by each shape's data-tone.
 */

import { ClickReport, Flick } from '../../api';
import { TOKENS } from '../../tokens/tokens';

/** A kill time over this is drawn in the attention color, in seconds. */
const LONG_KILL = 1;
/** The kill time that fills the bars' lane, in seconds: longer ones are cut at the top. */
const TALLEST_KILL = 2;
/** A bar is at least this tall, in CSS pixels, so the quickest kill still shows. */
const MIN_BAR = 1;
/** A kill's mark is this wide, in CSS pixels. */
const MARK_WIDTH = 2;

/** The lanes' sizes, in CSS pixels. */
interface LaneSizes {
  /** The lanes' height. */
  height: number;
  /** Where the marks' lane ends and the bars' lane begins, from the top. */
  split: number;
  /** A kill mark's height. */
  mark: number;
  /** A TTK bar's width. */
  bar: number;
}

/** The lanes' sizes, from the tokens. */
const SIZES: LaneSizes = {
  height: parseFloat(TOKENS.dark['kill-lanes-height']),
  split: Number(TOKENS.dark['kill-lanes-split']),
  mark: Number(TOKENS.dark['kill-lanes-mark']),
  bar: Number(TOKENS.dark['kill-lanes-bar']),
};

/** How a kill's bar is colored: the picked one apart, a long kill in the attention color. */
export type BarTone = 'picked' | 'long' | 'quiet';

/** One kill's shapes. */
export interface LaneKill {
  /** The kill. */
  flick: Flick;
  /** Its moment, as a percent of the lanes' width. */
  atPercent: number;
  /** Whether it is the picked kill (its mark drawn apart). */
  picked: boolean;
  /** Its bar's top, in CSS pixels from the lanes' top. */
  barTop: number;
  /** Its bar's height, in CSS pixels. */
  barHeight: number;
  /** Its bar's color. */
  tone: BarTone;
}

/** The lanes' shapes. */
export interface LanesModel {
  /** The line between the lanes, in CSS pixels from the top. */
  split: number;
  /** The marks' top, in CSS pixels: they are centered in their lane. */
  markTop: number;
  /** A mark's size, in CSS pixels. */
  markWidth: number;
  /** A mark's height, in CSS pixels. */
  markHeight: number;
  /** A bar's width, in CSS pixels. */
  barWidth: number;
  /** Each kill's shapes, in the report's order. */
  kills: LaneKill[];
}

/** A kill's bar color: the picked one apart, a long kill in the attention color. */
function toneOf(kill: Flick, picked: Flick | null): BarTone {
  if (kill === picked) return 'picked';
  return kill.total > LONG_KILL ? 'long' : 'quiet';
}

/** The lanes' shapes for a run `seconds` long, with `picked` drawn apart. */
export function killLanes(report: ClickReport, seconds: number, picked: Flick | null): LanesModel {
  const barsHeight = SIZES.height - SIZES.split - 1;
  return {
    split: SIZES.split,
    markTop: (SIZES.split - SIZES.mark) / 2,
    markWidth: MARK_WIDTH,
    markHeight: SIZES.mark,
    barWidth: SIZES.bar,
    kills: report.flicks.map((kill) => {
      const barHeight = Math.max(
        MIN_BAR,
        (Math.min(kill.total, TALLEST_KILL) / TALLEST_KILL) * barsHeight,
      );
      return {
        flick: kill,
        atPercent: (100 * kill.kill_frame) / report.fps / seconds,
        picked: kill === picked,
        barTop: SIZES.height - barHeight,
        barHeight,
        tone: toneOf(kill, picked),
      };
    }),
  };
}
