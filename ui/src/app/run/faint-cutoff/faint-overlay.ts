import { Report, Tracks } from '../../api';
import { FaintHover } from '../../services/faint-cutoff';
import { toPx } from '../track';

/** How the cut-off draws on the video: its colors, font and sizes, from its tokens (themes/faint.scss). */
export interface FaintStyle {
  font: string;
  ring: string;
  picked: string;
  radius: number;
  labelBg: string;
  hoverBg: string;
  text: string;
  textOut: string;
}

export function readFaintStyle(el: Element): FaintStyle {
  const css = getComputedStyle(el);
  const v = (name: string) => css.getPropertyValue(name).trim();
  return {
    font: v('--overlay-font'),
    ring: v('--overlay-faint-ring'),
    picked: v('--overlay-faint-picked'),
    radius: Number(v('--overlay-faint-ring-radius')),
    labelBg: v('--overlay-faint-label-bg'),
    hoverBg: v('--overlay-faint-hover-bg'),
    text: v('--overlay-faint-text'),
    textOut: v('--overlay-faint-text-out'),
  };
}

/** What the cut-off shows on a frame: the tracks it leaves out, the one picked, the scores, and the point hovered. */
export interface FaintLayer {
  scores: ReadonlyMap<number, number>;
  dropped: ReadonlySet<number>;
  highlight: number | null;
  showScores: boolean;
  hover: FaintHover | null;
}

const DASH = [2, 3];
const LINE = 1.2;
const LINE_PICKED = 2;
const LABEL_X = 10;
const LABEL_Y = 16;
const LABEL_H = 14;
const LABEL_PAD = 3;
const HOVER_X = 12;
const HOVER_Y = 8;
const HOVER_H = 18;
const HOVER_PAD = 5;
/** How near the mouse a track is pointed at, in pixels. */
const POINT_RADIUS = 12;

/** A track's score as the strip writes it; – for a track too short to have one. */
function scoreText(v: number | undefined): string {
  return v === undefined ? '–' : v.toFixed(2);
}

/**
 * The tracks the cut-off leaves out, on the frame shown: dimmed dashed rings, so the user sees what it removes. With
 * the scores shown, every track's score beside it; the track picked in the strip highlighted; the point under the mouse
 * with that frame's own score.
 */
export function drawFaint(
  c: CanvasRenderingContext2D,
  r: Report,
  all: Tracks,
  frame: number,
  scale: number,
  st: FaintStyle,
  layer: FaintLayer,
): void {
  const f = all.frames[frame];
  if (!f) return;
  c.font = st.font;
  c.textBaseline = 'alphabetic';
  for (const [id, x, y] of f.t) {
    const [px, py] = toPx(r.geometry, x, y, scale);
    const out = layer.dropped.has(id);
    const picked = id === layer.highlight;
    if (out || picked) {
      c.strokeStyle = picked ? st.picked : st.ring;
      c.lineWidth = picked ? LINE_PICKED : LINE;
      c.setLineDash(picked ? [] : DASH);
      c.beginPath();
      c.arc(px, py, st.radius, 0, 2 * Math.PI);
      c.stroke();
      c.setLineDash([]);
    }
    if (layer.showScores || picked) {
      const text = scoreText(layer.scores.get(id));
      c.fillStyle = st.labelBg;
      c.fillRect(px + LABEL_X, py - LABEL_Y, c.measureText(text).width + 2 * LABEL_PAD, LABEL_H);
      c.fillStyle = out ? st.textOut : st.text;
      c.fillText(text, px + LABEL_X + LABEL_PAD, py - LABEL_PAD - 2);
    }
  }
  const h = layer.hover;
  if (h && h.frame === frame) {
    const width = c.measureText(h.text).width + 2 * HOVER_PAD;
    // to the point's left where the right of the video has no room for it
    const x = h.x + HOVER_X + width > c.canvas.clientWidth ? h.x - HOVER_X - width : h.x + HOVER_X;
    c.fillStyle = st.hoverBg;
    c.fillRect(x, h.y + HOVER_Y, width, HOVER_H);
    c.fillStyle = st.text;
    c.fillText(h.text, x + HOVER_PAD, h.y + HOVER_Y + HOVER_H - HOVER_PAD);
  }
}

/** The track the mouse points at on the frame shown (within 12 pixels), with its frame's score and its own. */
export function pointedTrack(
  r: Report,
  all: Tracks,
  frame: number,
  scale: number,
  mouseX: number,
  mouseY: number,
  scores: ReadonlyMap<number, number>,
): FaintHover | null {
  const f = all.frames[frame];
  if (!f) return null;
  let best: FaintHover | null = null;
  let bestD = Infinity;
  for (let k = 0; k < f.t.length; k++) {
    const [id, x, y] = f.t[k];
    const [px, py] = toPx(r.geometry, x, y, scale);
    const d = Math.hypot(px - mouseX, py - mouseY);
    if (d > POINT_RADIUS || d >= bestD) continue;
    bestD = d;
    const text = `track ${id} · this frame ${scoreText(f.s?.[k])} · track ${scoreText(scores.get(id))}`;
    best = { id, x: px, y: py, frame, text };
  }
  return best;
}
