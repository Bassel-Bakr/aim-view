import { ClickReport, Flick } from '../../api';

/** A kill time over this is drawn in the attention color, in seconds. */
const LONG_KILL = 1;
/** The kill time that fills the bars' lane, in seconds: longer ones are cut at the top. */
const TALLEST_KILL = 2;

/** The lanes' drawing: the kills' marks above, each kill's time as a bar below, both at the kill's moment. */
export interface LaneStyle {
  kill: string;
  picked: string;
  quiet: string;
  long: string;
  grid: string;
  markHeight: number;
  barWidth: number;
  split: number;
}

export function readStyle(element: Element): LaneStyle {
  const css = getComputedStyle(element);
  const token = (name: string) => css.getPropertyValue(name).trim();
  return {
    kill: token('--accent'),
    picked: token('--text-primary'),
    quiet: token('--series-quiet'),
    long: token('--attention'),
    grid: token('--grid'),
    markHeight: Number(token('--kill-lanes-mark')),
    barWidth: Number(token('--kill-lanes-bar')),
    split: Number(token('--kill-lanes-split')),
  };
}

/** A kill's mark is this wide, in pixels; a bar is at least this tall. */
const MARK_WIDTH = 2;
const MIN_BAR = 1;

/** A kill's bar: the picked one apart, a long kill in the attention color. */
function barColor(kill: Flick, picked: Flick | null, style: LaneStyle): string {
  if (kill === picked) return style.picked;
  return kill.total > LONG_KILL ? style.long : style.quiet;
}

/**
 * The lanes on their canvas (widthPx by heightPx, the run seconds long): the line between them, and for each kill its
 * mark above and its time as a bar below.
 */
export function drawKillLanes(
  context: CanvasRenderingContext2D,
  report: ClickReport,
  widthPx: number,
  heightPx: number,
  seconds: number,
  picked: Flick | null,
  style: LaneStyle,
): void {
  context.fillStyle = style.grid;
  context.fillRect(0, style.split, widthPx, 1);
  const markTop = (style.split - style.markHeight) / 2;
  const barsHeight = heightPx - style.split - 1;
  for (const kill of report.flicks) {
    const at = Math.round((kill.kill_frame / report.fps / seconds) * widthPx);
    context.fillStyle = kill === picked ? style.picked : style.kill;
    context.fillRect(at - MARK_WIDTH / 2, markTop, MARK_WIDTH, style.markHeight);
    const bar = Math.max(MIN_BAR, (Math.min(kill.total, TALLEST_KILL) / TALLEST_KILL) * barsHeight);
    context.fillStyle = barColor(kill, picked, style);
    context.fillRect(at - style.barWidth / 2, heightPx - bar, style.barWidth, bar);
  }
}
