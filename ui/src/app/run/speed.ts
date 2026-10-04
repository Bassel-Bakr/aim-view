import { PathPoint } from '../api';

/** The crosshair's speed at a frame: [frame, degrees per second]. */
export type SpeedPoint = [frame: number, speed: number];

/** The Gaussian's width for "Smooth", in seconds. */
const SMOOTH_SIGMA = 0.025;
/** Speeds further than this many sigmas away weigh too little to count. */
const SIGMA_REACH = 3;

/**
 * The crosshair's speed through a flick, from the target's path relative to the crosshair. Positions are averaged
 * over 3 frames first (the capture moves in uneven steps), then the speed is taken between neighbors. The first point
 * has no neighbor before it and is 0.
 */
export function speeds(path: PathPoint[], fps: number, smooth: boolean): SpeedPoint[] {
  const averaged = path.map((point, i): PathPoint => {
    if (i === 0 || i === path.length - 1) return point;
    return [
      point[0],
      (path[i - 1][1] + point[1] + path[i + 1][1]) / 3,
      (path[i - 1][2] + point[2] + path[i + 1][2]) / 3,
    ];
  });
  const raw = averaged.map((point, i): SpeedPoint => {
    if (!i) return [point[0], 0];
    const before = averaged[i - 1];
    return [
      point[0],
      (Math.hypot(point[1] - before[1], point[2] - before[2]) * fps) /
        Math.max(1, point[0] - before[0]),
    ];
  });
  return smooth ? smoothed(raw, SMOOTH_SIGMA * fps) : raw;
}

/** A Gaussian over the speeds, sigma in frames, weighted by frame distance; the first point (a placeholder) is left out. */
export function smoothed(data: SpeedPoint[], sigma: number): SpeedPoint[] {
  return data.map(([frame]): SpeedPoint => {
    let sum = 0;
    let weight = 0;
    for (let i = 1; i < data.length; i++) {
      const offset = data[i][0] - frame;
      if (Math.abs(offset) > SIGMA_REACH * sigma) continue;
      const weightHere = Math.exp(-(offset * offset) / (2 * sigma * sigma));
      sum += weightHere * data[i][1];
      weight += weightHere;
    }
    return [frame, weight ? sum / weight : 0];
  });
}
