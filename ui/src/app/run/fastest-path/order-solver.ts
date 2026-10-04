import { Flick, TrackPoint } from '../../api';

/**
 * Fitts' law fitted to a run's kills: a flick takes a + b * log2(1 + D / W) seconds, D its distance and W the target's
 * width.
 */
export interface Fitts {
  a: number;
  b: number;
  widthDeg: number;
}

/** Fewer kills than this give no fit: a flat 0.1 s per unit is used. */
const MIN_FIT = 3;
const DEFAULT_B = 0.1;

export function fitFitts(flicks: Flick[], radius: number): Fitts {
  const widthDeg = 2 * radius;
  const samples = flicks
    .filter((kill) => kill.total != null && kill.D0 > 0)
    .map((kill) => [Math.log2(1 + kill.D0 / widthDeg), kill.total]);
  const count = samples.length;
  if (count < MIN_FIT) return { a: 0, b: DEFAULT_B, widthDeg };
  const meanUnits = samples.reduce((sum, sample) => sum + sample[0], 0) / count;
  const meanSeconds = samples.reduce((sum, sample) => sum + sample[1], 0) / count;
  const sumProducts = samples.reduce(
    (sum, sample) => sum + (sample[0] - meanUnits) * (sample[1] - meanSeconds),
    0,
  );
  const sumSquares = samples.reduce((sum, sample) => sum + (sample[0] - meanUnits) ** 2, 0);
  const b = sumSquares > 0 && sumProducts > 0 ? sumProducts / sumSquares : DEFAULT_B;
  return { a: meanSeconds - b * meanUnits, b, widthDeg };
}

/**
 * For a set of targets: best[mask * targetCount + j], the least cost to visit every target in mask starting at j, and next[...]
 * the target after j. It depends only on the targets' places relative to each other, which hold while the view moves.
 */
interface OrderTable {
  targetCount: number;
  fullMask: number;
  best: Float64Array;
  next: Int8Array;
}

/** A table kept for one set of targets (key: their sorted ids), in the order of ids. */
interface CachedTable {
  key: string;
  ids: number[];
  table: OrderTable;
}

/** The best orders through a set: the targets (in the table's order), from[j] (the whole set's cost when j goes first), and bestFirst, the best j. */
export interface Solution {
  targets: TrackPoint[];
  table: OrderTable;
  from: number[];
  bestFirst: number;
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
  costFromCrosshair(target: TrackPoint): number {
    return Math.log2(1 + Math.hypot(target[1], target[2]) / this.fitts.widthDeg);
  }

  /** From one target to another, in log units. */
  cost(from: TrackPoint, to: TrackPoint): number {
    return Math.log2(1 + Math.hypot(from[1] - to[1], from[2] - to[2]) / this.fitts.widthDeg);
  }

  /** The time for n flicks of so many log units in all. */
  seconds(flickCount: number, units: number): number {
    return flickCount * this.fitts.a + this.fitts.b * units;
  }

  /** The log units of a path from the crosshair through the targets in order. */
  pathUnits(order: TrackPoint[]): number {
    return order.reduce(
      (units, target, i) =>
        units + (i ? this.cost(order[i - 1], target) : this.costFromCrosshair(target)),
      0,
    );
  }

  solve(targets: TrackPoint[]): Solution | null {
    if (!targets.length) return null;
    const candidates =
      targets.length > MAX_TARGETS
        ? [...targets]
            .sort((a, b) => this.costFromCrosshair(a) - this.costFromCrosshair(b))
            .slice(0, MAX_TARGETS)
        : targets;
    const key = candidates
      .map((target) => target[0])
      .sort((a, b) => a - b)
      .join(',');
    if (this.cache?.key !== key)
      this.cache = {
        key,
        ids: candidates.map((target) => target[0]),
        table: this.table(candidates),
      };
    const { ids, table } = this.cache;
    const byId = new Map(candidates.map((target) => [target[0], target]));
    const inTableOrder = ids.map((id) => byId.get(id) as TrackPoint);
    const from = inTableOrder.map(
      (target, j) =>
        this.costFromCrosshair(target) + table.best[table.fullMask * table.targetCount + j],
    );
    return { targets: inTableOrder, table, from, bestFirst: from.indexOf(Math.min(...from)) };
  }

  fastestOrder(targets: TrackPoint[]): FastestOrder | null {
    const solution = this.solve(targets);
    if (!solution) return null;
    const { targets: inTableOrder, table } = solution;
    const order: TrackPoint[] = [];
    for (let mask = table.fullMask, j = solution.bestFirst; j >= 0;) {
      order.push(inTableOrder[j]);
      const after = table.next[mask * table.targetCount + j];
      mask ^= 1 << j;
      j = after;
    }
    return { order, seconds: this.seconds(order.length, solution.from[solution.bestFirst]) };
  }

  private table(targets: TrackPoint[]): OrderTable {
    const count = targets.length;
    const fullMask = (1 << count) - 1;
    const best = new Float64Array((fullMask + 1) * count).fill(Infinity);
    const next = new Int8Array((fullMask + 1) * count).fill(-1);
    for (let j = 0; j < count; j++) best[(1 << j) * count + j] = 0;
    for (let mask = 1; mask <= fullMask; mask++) {
      for (let j = 0; j < count; j++) {
        if (!(mask & (1 << j)) || mask === 1 << j) continue;
        const rest = mask ^ (1 << j);
        for (let after = 0; after < count; after++) {
          if (!(rest & (1 << after))) continue;
          const cost = this.cost(targets[j], targets[after]) + best[rest * count + after];
          if (cost < best[mask * count + j]) {
            best[mask * count + j] = cost;
            next[mask * count + j] = after;
          }
        }
      }
    }
    return { targetCount: count, fullMask, best, next };
  }
}
