import { Flick, TrackPoint } from '../../api';

/**
 * Fitts' law fitted to a run's kills: a flick takes a + b * log2(1 + D / W) seconds, D its distance and W the target's
 * width.
 */
export interface Fitts {
  a: number;
  b: number;
  W: number;
}

/** Fewer kills than this give no fit: a flat 0.1 s per unit is used. */
const MIN_FIT = 3;
const DEFAULT_B = 0.1;

export function fitFitts(flicks: Flick[], radius: number): Fitts {
  const W = 2 * radius;
  const pts = flicks
    .filter((m) => m.total != null && m.D0 > 0)
    .map((m) => [Math.log2(1 + m.D0 / W), m.total]);
  const n = pts.length;
  if (n < MIN_FIT) return { a: 0, b: DEFAULT_B, W };
  const mx = pts.reduce((s, p) => s + p[0], 0) / n;
  const my = pts.reduce((s, p) => s + p[1], 0) / n;
  const sxy = pts.reduce((s, p) => s + (p[0] - mx) * (p[1] - my), 0);
  const sxx = pts.reduce((s, p) => s + (p[0] - mx) ** 2, 0);
  const b = sxx > 0 && sxy > 0 ? sxy / sxx : DEFAULT_B;
  return { a: my - b * mx, b, W };
}

/**
 * For a set of targets: best[mask * n + j], the least cost to visit every target in mask starting at j, and next[...]
 * the target after j. It depends only on the targets' places relative to each other, which hold while the view moves.
 */
interface OrderTable {
  n: number;
  full: number;
  best: Float64Array;
  next: Int8Array;
}

/** A table kept for one set of targets (key: their sorted ids), in the order of ids. */
interface CachedTable {
  key: string;
  ids: number[];
  table: OrderTable;
}

/** The best orders through a set: the targets (in the table's order), from[j] (the whole set's cost when j goes first), and j0 the best first. */
export interface Solution {
  pts: TrackPoint[];
  table: OrderTable;
  from: number[];
  j0: number;
}

/** The fastest order through the targets, and its predicted time in seconds. */
export interface FastestOrder {
  order: TrackPoint[];
  seconds: number;
}

/** Over this many targets, only the ones cheapest to reach are ordered (the table doubles with each one). */
const MAX_TARGETS = 14;

/**
 * Orders targets by Fitts' law. Costs are in log units (seconds = a per flick + b per unit). The table for a set of
 * targets is kept until the set changes, so each frame only adds the flick from the crosshair.
 */
export class OrderSolver {
  private cache: CachedTable | null = null;

  constructor(readonly fitts: Fitts) {}

  /** From the crosshair to a target, in log units. */
  costFromCrosshair(p: TrackPoint): number {
    return Math.log2(1 + Math.hypot(p[1], p[2]) / this.fitts.W);
  }

  /** From one target to another, in log units. */
  cost(p: TrackPoint, q: TrackPoint): number {
    return Math.log2(1 + Math.hypot(p[1] - q[1], p[2] - q[2]) / this.fitts.W);
  }

  /** The time for n flicks of so many log units in all. */
  seconds(n: number, units: number): number {
    return n * this.fitts.a + this.fitts.b * units;
  }

  /** The log units of a path from the crosshair through the targets in order. */
  pathUnits(order: TrackPoint[]): number {
    return order.reduce(
      (u, p, i) => u + (i ? this.cost(order[i - 1], p) : this.costFromCrosshair(p)),
      0,
    );
  }

  solve(targets: TrackPoint[]): Solution | null {
    if (!targets.length) return null;
    const ts =
      targets.length > MAX_TARGETS
        ? [...targets]
            .sort((p, q) => this.costFromCrosshair(p) - this.costFromCrosshair(q))
            .slice(0, MAX_TARGETS)
        : targets;
    const key = ts
      .map((t) => t[0])
      .sort((a, b) => a - b)
      .join(',');
    if (this.cache?.key !== key)
      this.cache = { key, ids: ts.map((t) => t[0]), table: this.table(ts) };
    const { ids, table } = this.cache;
    const byId = new Map(ts.map((t) => [t[0], t]));
    const pts = ids.map((i) => byId.get(i) as TrackPoint);
    const from = pts.map(
      (p, j) => this.costFromCrosshair(p) + table.best[table.full * table.n + j],
    );
    return { pts, table, from, j0: from.indexOf(Math.min(...from)) };
  }

  fastestOrder(targets: TrackPoint[]): FastestOrder | null {
    const sol = this.solve(targets);
    if (!sol) return null;
    const { pts, table } = sol;
    const order: TrackPoint[] = [];
    for (let mask = table.full, j = sol.j0; j >= 0;) {
      order.push(pts[j]);
      const k = table.next[mask * table.n + j];
      mask ^= 1 << j;
      j = k;
    }
    return { order, seconds: this.seconds(order.length, sol.from[sol.j0]) };
  }

  private table(ts: TrackPoint[]): OrderTable {
    const n = ts.length;
    const full = (1 << n) - 1;
    const best = new Float64Array((full + 1) * n).fill(Infinity);
    const next = new Int8Array((full + 1) * n).fill(-1);
    for (let j = 0; j < n; j++) best[(1 << j) * n + j] = 0;
    for (let mask = 1; mask <= full; mask++) {
      for (let j = 0; j < n; j++) {
        if (!(mask & (1 << j)) || mask === 1 << j) continue;
        const rest = mask ^ (1 << j);
        for (let k = 0; k < n; k++) {
          if (!(rest & (1 << k))) continue;
          const v = this.cost(ts[j], ts[k]) + best[rest * n + k];
          if (v < best[mask * n + j]) {
            best[mask * n + j] = v;
            next[mask * n + j] = k;
          }
        }
      }
    }
    return { n, full, best, next };
  }
}
