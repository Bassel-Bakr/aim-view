import { PathPoint } from '../api';

/** The crosshair's speed at a frame: [frame, degrees per second]. */
export type SpeedPoint = [frame: number, speed: number];

/** The Gaussian's width for "Smooth", in seconds. */
const SMOOTH_SIGMA = 0.025;

/**
 * The crosshair's speed through a flick, from the target's path relative to the crosshair. Positions are averaged
 * over 3 frames first (the capture moves in uneven steps), then the speed is taken between neighbors. The first point
 * has no neighbor before it and is 0.
 */
export function speeds(path: PathPoint[], fps: number, smooth: boolean): SpeedPoint[] {
  const pts = path.map((q, i): PathPoint => {
    if (i === 0 || i === path.length - 1) return q;
    return [
      q[0],
      (path[i - 1][1] + q[1] + path[i + 1][1]) / 3,
      (path[i - 1][2] + q[2] + path[i + 1][2]) / 3,
    ];
  });
  const raw = pts.map((q, i): SpeedPoint => {
    if (!i) return [q[0], 0];
    const p = pts[i - 1];
    return [q[0], (Math.hypot(q[1] - p[1], q[2] - p[2]) * fps) / Math.max(1, q[0] - p[0])];
  });
  return smooth ? smoothed(raw, SMOOTH_SIGMA * fps) : raw;
}

/** A Gaussian over the speeds, sigma in frames, weighted by frame distance; the first point (a placeholder) is left out. */
export function smoothed(data: SpeedPoint[], sigma: number): SpeedPoint[] {
  return data.map(([f]): SpeedPoint => {
    let sum = 0;
    let weight = 0;
    for (let i = 1; i < data.length; i++) {
      const d = data[i][0] - f;
      if (Math.abs(d) > 3 * sigma) continue;
      const k = Math.exp(-(d * d) / (2 * sigma * sigma));
      sum += k * data[i][1];
      weight += k;
    }
    return [f, weight ? sum / weight : 0];
  });
}
