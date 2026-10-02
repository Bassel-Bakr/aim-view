import { ClickReport, TrackPoint, TrackReport, Tracks } from '../../api';
import { formatMs } from '../../format';
import { NEW_MS, newCut, PathAnalysis } from '../fastest-path/path-analysis';
import { boxes, flickAt, nearest, Point, toPx } from '../track';

/** How the overlay draws: its colors, font and line widths, from the player's tokens (themes/player.scss). */
export interface OverlayStyle {
  font: string;
  crosshair: string;
  otherTarget: string;
  ring: string;
  onTarget: string;
  offTarget: string;
  labelBg: string;
  labelText: string;
  line: number;
  lineStrong: number;
  fastest: string;
  mine: string;
  orderFont: string;
  pathWidth: number;
  legendBg: string;
}

export function readOverlayStyle(el: Element): OverlayStyle {
  const css = getComputedStyle(el);
  const v = (name: string) => css.getPropertyValue(name).trim();
  return {
    font: v('--overlay-font'),
    crosshair: v('--overlay-crosshair'),
    otherTarget: v('--overlay-other-target'),
    ring: v('--overlay-ring'),
    onTarget: v('--on-target'),
    offTarget: v('--off-target'),
    labelBg: v('--overlay-label-bg'),
    labelText: v('--overlay-label-text'),
    line: Number(v('--overlay-line-width')),
    lineStrong: Number(v('--overlay-line-width-strong')),
    fastest: v('--overlay-fastest'),
    mine: v('--overlay-mine'),
    orderFont: v('--overlay-order-font'),
    pathWidth: Number(v('--overlay-path-width')),
    legendBg: v('--overlay-legend-bg'),
  };
}

/** Frames before a flick starts and after its kill that the overlay still shows it. */
const NEAR_FLICK = 30;
const CROSSHAIR_RADIUS = 10;
const DASH = [4, 4];
/** The ring round a clicking run's target, as a share of the target's radius, and its smallest radius in pixels. */
const RING_SCALE = 1.8;
const RING_MIN = 6;
/** A tracked box's gap from the target's edge, and a label's padding, in pixels. */
const BOX_PAD = 1.5;
const LABEL_PAD = 4;
const LABEL_HEIGHT = 16;

function label(c: CanvasRenderingContext2D, st: OverlayStyle, text: string, [x, y]: Point): void {
  c.font = st.font;
  c.fillStyle = st.labelBg;
  c.fillRect(x, y - LABEL_PAD, c.measureText(text).width + 2 * LABEL_PAD, LABEL_HEIGHT);
  c.fillStyle = st.labelText;
  c.textBaseline = 'middle';
  c.fillText(text, x + LABEL_PAD, y - LABEL_PAD + LABEL_HEIGHT / 2);
}

function dashedLine(c: CanvasRenderingContext2D, [x0, y0]: Point, [x1, y1]: Point): void {
  c.setLineDash(DASH);
  c.beginPath();
  c.moveTo(x0, y0);
  c.lineTo(x1, y1);
  c.stroke();
  c.setLineDash([]);
}

/**
 * A clicking run: the crosshair as a faint ring, and the flick's target as a ring with a dashed line from the
 * crosshair and its distance, from just before the flick until just after its kill.
 */
export function drawClick(
  c: CanvasRenderingContext2D,
  r: ClickReport,
  frame: number,
  scale: number,
  st: OverlayStyle,
): void {
  const f = flickAt(r.flicks, frame);
  if (!f || frame < f.start_frame - NEAR_FLICK || frame > f.kill_frame + NEAR_FLICK) return;
  const g = r.geometry;
  const center = toPx(g, 0, 0, scale);
  c.strokeStyle = st.crosshair;
  c.lineWidth = st.line;
  c.beginPath();
  c.arc(center[0], center[1], CROSSHAIR_RADIUS, 0, 2 * Math.PI);
  c.stroke();
  const p = r.paths[String(f.n)]?.find((q) => q[0] === frame);
  if (!p) return;
  const target = toPx(g, p[1], p[2], scale);
  const edge = toPx(g, p[1] + r.summary.radius, p[2], scale)[0];
  const radius = Math.max(RING_MIN, (edge - target[0]) * RING_SCALE);
  c.strokeStyle = st.ring;
  c.lineWidth = st.lineStrong;
  c.beginPath();
  c.arc(target[0], target[1], radius, 0, 2 * Math.PI);
  c.stroke();
  dashedLine(c, center, target);
  const distance = `${Math.hypot(p[1], p[2]).toFixed(1)}°`;
  label(c, st, distance, [target[0] + radius + LABEL_PAD, target[1]]);
}

/**
 * A tracking run: every target as a box, the one the review follows (nearest the crosshair) green while the crosshair
 * is on it and orange with a dashed line and its distance when not; "switching" after a bot's death.
 */
export function drawTrack(
  c: CanvasRenderingContext2D,
  r: TrackReport,
  tracks: Tracks,
  frame: number,
  scale: number,
  st: OverlayStyle,
): void {
  const s = r.summary;
  const f = tracks.frames[frame];
  if (!f || (s.start !== null && s.end !== null && (frame < s.start || frame >= s.end))) return;
  const g = r.geometry;
  const bs = boxes(f);
  const best = nearest(bs);
  let corner: Point | null = null;
  for (const b of bs) {
    const [x0, y0] = toPx(g, b.x - b.w / 2, b.y + b.h / 2, scale);
    const [x1, y1] = toPx(g, b.x + b.w / 2, b.y - b.h / 2, scale);
    const main = b === best;
    c.strokeStyle = main ? (b.inside ? st.onTarget : st.offTarget) : st.otherTarget;
    c.lineWidth = main ? st.lineStrong : st.line;
    c.strokeRect(x0 - BOX_PAD, y0 - BOX_PAD, x1 - x0 + 2 * BOX_PAD, y1 - y0 + 2 * BOX_PAD);
    if (main) corner = [x1, y1];
  }
  const center = toPx(g, 0, 0, scale);
  if (best && !best.inside) {
    c.strokeStyle = st.offTarget;
    c.lineWidth = st.line;
    dashedLine(c, center, toPx(g, best.x, best.y, scale));
  }
  const switching = s.switches.some(([a, b]) => frame >= a && frame < b);
  let text = '';
  if (switching) text = 'switching';
  else if (best) text = best.inside ? 'on' : `${best.out.toFixed(2)}° off its edge`;
  if (text) {
    const [x, y] = corner ?? center;
    label(c, st, text, [x + LABEL_PAD + BOX_PAD, y]);
  }
}

/** Which of the two paths to draw. */
export interface PathFlags {
  fastest: boolean;
  mine: boolean;
}

/** A line of the paths' legend: its color and text. */
interface LegendRow {
  color: string;
  text: string;
}

/** The dots along a path: round, small enough to see past, and numbered. */
const DOT_RADIUS = 5;
const DOT_DASH = [0.1, 3];
const PATH_ALPHA = 0.7;
const DOT_ALPHA = 0.8;
/** The two paths' numbers sit to either side of a target, so both show on a shared one. */
const MINE_OFFSET: Point = [-8, 8];
const FASTEST_OFFSET: Point = [8, -8];
const LEGEND_X = 6;
const LEGEND_ROW = 15;
const LEGEND_SWATCH = 7;

function path(
  c: CanvasRenderingContext2D,
  r: ClickReport,
  scale: number,
  st: OverlayStyle,
  order: TrackPoint[],
  color: string,
  [dx, dy]: Point,
): void {
  const g = r.geometry;
  const px = order.map((p) => toPx(g, p[1], p[2], scale));
  c.strokeStyle = color;
  c.lineWidth = st.pathWidth;
  c.lineCap = 'round';
  c.setLineDash(DOT_DASH);
  c.globalAlpha = PATH_ALPHA;
  c.beginPath();
  const [cx, cy] = toPx(g, 0, 0, scale);
  c.moveTo(cx, cy);
  for (const [x, y] of px) c.lineTo(x, y);
  c.stroke();
  c.lineCap = 'butt';
  c.setLineDash([]);
  c.globalAlpha = DOT_ALPHA;
  c.font = st.orderFont;
  c.textAlign = 'center';
  c.textBaseline = 'middle';
  px.forEach(([x, y], i) => {
    c.fillStyle = color;
    c.beginPath();
    c.arc(x + dx, y + dy, DOT_RADIUS, 0, 2 * Math.PI);
    c.fill();
    c.fillStyle = st.labelText;
    c.fillText(String(i + 1), x + dx, y + dy);
  });
  c.globalAlpha = 1;
  c.textAlign = 'left';
  c.textBaseline = 'alphabetic';
}

/**
 * The fastest order through the targets on screen (green), and the order you killed them in (orange), each with its
 * predicted time in a legend. Only during the run: not on the countdown before it, nor after the last kill. Targets new
 * to the screen are left out, as in the path cost.
 */
export function drawPaths(
  c: CanvasRenderingContext2D,
  r: ClickReport,
  tracks: Tracks,
  frame: number,
  scale: number,
  st: OverlayStyle,
  a: PathAnalysis,
  show: PathFlags,
): void {
  const f = tracks.frames[frame];
  if (!f || frame > a.lastKill || frame < a.firstStart) return;
  const m = flickAt(r.flicks, frame);
  const cut = m && m.kill_frame >= frame ? newCut(m, r.fps) : frame - (NEW_MS / 1000) * r.fps;
  const fresh = f.t.filter((t) => (a.firstSeen.get(t[0]) ?? Infinity) <= cut);
  // only new targets on screen (one at a time): no choice to leave out
  const ts = fresh.length ? fresh : f.t;
  const skipped = f.t.length - ts.length;
  const best = a.solver.fastestOrder(ts);
  if (!best) return;
  const yours = ts
    .filter((t) => (a.killOf.get(t[0])?.kill_frame ?? -1) >= frame)
    .sort((x, y) => (a.killOf.get(x[0])?.kill_frame ?? 0) - (a.killOf.get(y[0])?.kill_frame ?? 0));
  const showMine = show.mine && yours.length > 0;
  const n = best.order.length;
  const targets = `the ${n} target${n > 1 ? 's' : ''} on screen`;
  const rows: LegendRow[] = [];
  if (showMine) path(c, r, scale, st, yours, st.mine, MINE_OFFSET);
  if (show.fastest) {
    path(c, r, scale, st, best.order, st.fastest, FASTEST_OFFSET);
    const left = skipped ? ` (${skipped} new left out)` : '';
    rows.push({
      color: st.fastest,
      text: `Fastest path through ${targets}${left}: about ${formatMs(best.seconds)}`,
    });
  }
  if (showMine) {
    const mine = a.solver.seconds(yours.length, a.solver.pathUnits(yours));
    const ref = yours.length === n ? best.seconds : (a.solver.fastestOrder(yours)?.seconds ?? mine);
    const slower = Math.round(1000 * (mine - ref));
    const what = show.fastest ? 'them' : targets;
    const through =
      yours.length === n
        ? `Your path through ${what}`
        : `Your path through the ${yours.length} of ${what} you killed`;
    rows.push({
      color: st.mine,
      text: `${through}: about ${formatMs(mine)}, ${slower <= 0 ? 'the fastest' : `${slower} ms slower than the fastest`}`,
    });
  }
  if (!rows.length) return;
  c.font = st.font;
  const width = Math.max(...rows.map((row) => c.measureText(row.text).width)) + 4 * LEGEND_X;
  c.fillStyle = st.legendBg;
  c.fillRect(LEGEND_X, LEGEND_X, width, LEGEND_X + LEGEND_ROW * rows.length);
  rows.forEach((row, i) => {
    const y = 2 * LEGEND_X + LEGEND_ROW * i;
    c.fillStyle = row.color;
    c.fillRect(2 * LEGEND_X, y + 1, LEGEND_SWATCH, LEGEND_SWATCH);
    c.fillStyle = st.labelText;
    c.textBaseline = 'top';
    c.fillText(row.text, 2 * LEGEND_X + LEGEND_SWATCH + LEGEND_X, y);
  });
  c.textBaseline = 'alphabetic';
}
