import { Timeline, TrackState } from '../track';

/** How the timeline draws: colors, font and the strip's height, from its tokens (themes/timeline.scss). */
export interface TimelineStyle {
  grid: string;
  on: string;
  off: string;
  switching: string;
  text: string;
  labelBg: string;
  death: string;
  font: string;
  strip: number;
}

export function readTimelineStyle(el: Element): TimelineStyle {
  const css = getComputedStyle(el);
  const v = (name: string) => css.getPropertyValue(name).trim();
  return {
    grid: v('--grid'),
    on: v('--on-target'),
    off: v('--off-target'),
    switching: v('--text-muted'),
    text: v('--text-secondary'),
    labelBg: v('--overlay-label-bg'),
    death: v('--timeline-death'),
    font: v('--timeline-font'),
    strip: Number(v('--timeline-strip-height')),
  };
}

/** Space above the chart, between it and the strip, and under the strip for the seconds, in pixels. */
const TOP = 6;
const STRIP_GAP = 8;
const AXIS = 30;
const DASH = [3, 4];
const MAX_ALPHA = 0.35;
const MEAN_ALPHA = 0.9;
const LABEL_PAD = 3;
const LABEL_HEIGHT = 14;

/** The frames a pixel column covers. */
function columns(
  tl: Timeline,
  w: number,
  each: (k0: number, k1: number, px: number) => void,
): void {
  for (let px = 0; px < w; px++) {
    const k0 = Math.floor((px * tl.frameCount) / w);
    each(k0, Math.max(k0 + 1, Math.floor(((px + 1) * tl.frameCount) / w)), px);
  }
}

/**
 * The chart: how far outside the bot's edge the crosshair was, per pixel column the furthest (light) and the average
 * (dark); under it a strip of on target, off target and switching (the state most of the column's frames had), the
 * bots' deaths, and the seconds.
 */
export function drawTimeline(
  c: CanvasRenderingContext2D,
  tl: Timeline,
  w: number,
  h: number,
  st: TimelineStyle,
): void {
  const chart = h - TOP - STRIP_GAP - st.strip - AXIS;
  const stripY = TOP + chart + STRIP_GAP;
  const y = (d: number) => TOP + chart * (1 - Math.min(d, tl.capDeg) / tl.capDeg);
  c.clearRect(0, 0, w, h);
  c.strokeStyle = st.grid;
  for (const g of [0, 0.5, 1]) {
    c.setLineDash(g ? DASH : []);
    c.beginPath();
    c.moveTo(0, Math.round(y(g * tl.capDeg)) + 0.5);
    c.lineTo(w, Math.round(y(g * tl.capDeg)) + 0.5);
    c.stroke();
  }
  c.setLineDash([]);
  const mean: number[] = [];
  const max: number[] = [];
  columns(tl, w, (k0, k1) => {
    let sum = 0;
    let count = 0;
    let hi = 0;
    for (let k = k0; k < k1; k++) {
      const d = tl.outsideDeg[k];
      if (Number.isNaN(d)) continue;
      sum += d;
      count++;
      hi = Math.max(hi, d);
    }
    mean.push(count ? sum / count : 0);
    max.push(hi);
  });
  const area = (values: number[], alpha: number) => {
    c.fillStyle = st.off;
    c.globalAlpha = alpha;
    c.beginPath();
    c.moveTo(0, y(0));
    values.forEach((v, px) => {
      c.lineTo(px, y(v));
      c.lineTo(px + 1, y(v));
    });
    c.lineTo(w, y(0));
    c.closePath();
    c.fill();
    c.globalAlpha = 1;
  };
  area(max, MAX_ALPHA);
  area(mean, MEAN_ALPHA);
  const fills: Record<TrackState, string> = {
    [TrackState.NoBot]: st.grid,
    [TrackState.On]: st.on,
    [TrackState.Off]: st.off,
    [TrackState.Switching]: st.switching,
  };
  columns(tl, w, (k0, k1, px) => {
    const count = [0, 0, 0, 0];
    for (let k = k0; k < k1; k++) count[tl.state[k]]++;
    c.fillStyle = fills[count.indexOf(Math.max(...count)) as TrackState];
    c.fillRect(px, stripY, 1, st.strip);
  });
  c.fillStyle = st.death;
  for (const d of tl.deaths)
    c.fillRect(Math.floor((d / tl.frameCount) * w), stripY - 4, 1.5, st.strip + 4);
  c.font = st.font;
  c.textBaseline = 'middle';
  for (const [text, ty] of [
    [`${tl.capDeg.toFixed(1)}° off`, TOP + LABEL_HEIGHT / 2],
    ['0°: on the bot', TOP + chart - LABEL_HEIGHT / 2],
  ] as const) {
    c.fillStyle = st.labelBg;
    c.fillRect(0, ty - LABEL_HEIGHT / 2, c.measureText(text).width + 2 * LABEL_PAD, LABEL_HEIGHT);
    c.fillStyle = st.text;
    c.fillText(text, LABEL_PAD, ty);
  }
  c.textBaseline = 'alphabetic';
  const seconds = tl.frameCount / tl.fps;
  const step = seconds > 90 ? 20 : 10;
  for (let t = 0; t <= seconds; t += step) {
    const x = (t / seconds) * w;
    c.textAlign = t === 0 ? 'left' : x > w - 20 ? 'right' : 'center';
    c.fillText(`${t} s`, Math.min(w - 1, x), h - LABEL_PAD);
  }
  c.textAlign = 'left';
}
