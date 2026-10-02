import { ClickReport, TrackReport, Tracks } from '../../api';
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
