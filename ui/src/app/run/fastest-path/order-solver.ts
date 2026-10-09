/**
 * The fastest order to kill the targets on screen, by Fitts' law fitted to the run's own flicks.
 * In: the clicking report's flicks and target radius, and each frame's targets from the tracks.
 * Out: the costs and orders path-analysis.ts works out the Pathing check and overlays from.
 */

import { Flick, TrackPoint } from '../../api';

/**
 * Fitts' law fitted to a run's kills: a flick takes a + b * log2(1 + D / W) seconds, D its distance
 * and W the target's width.
 */
export interface Fitts {
  /** The fixed time a flick takes, in seconds (the fit's intercept). */
  a: number;
  /** The seconds each log unit of difficulty adds (the fit's slope). */
  b: number;
  /** The targets' width W, in degrees: twice the report's target radius. */
  widthDeg: number;
}

/** Fewer kills than this give no fit: a flat 0.1 s per unit is used. */
const MIN_FIT = 3;
/** The slope used without a fit, or when the fit's slope is not above 0, in seconds a log unit. */
const DEFAULT_B = 0.1;

/**
 * Fits Fitts' law by least squares to the flicks with a time and a start distance above 0: their
 * total time in seconds against log2(1 + D0 / W). `radius` is the targets' radius in degrees.
 */
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
 * For a set of targets: best[mask * targetCount + j], the least cost to visit every target in mask
 * starting at j, and next[...] the target after j. It depends only on the targets' places relative
 * to each other, which hold while the view moves.
 */
interface OrderTable {
  /** How many targets the table orders. */
  targetCount: number;
  /** The mask with every target's bit set. */
  fullMask: number;
  /**
   * The least cost in log units to visit a mask's targets starting at j; Infinity where j is not
   * in the mask.
   */
  best: Float64Array;
  /** The target after j on that best path; -1 at its end. */
  next: Int8Array;
}

/** A table kept for one set of targets, in the order of ids. */
interface CachedTable {
  /** The targets' ids in the table's order. */
  ids: number[];
  /** The table for those targets. */
  table: OrderTable;
}

/**
 * The best orders through a set: the targets (in the table's order), from[j] (the whole set's cost
 * when j goes first), and bestFirst, the best j.
 */
export interface Solution {
  /** The targets ordered, in the table's order. */
  targets: TrackPoint[];
  /** The table of best paths through them. */
  table: OrderTable;
  /** For each target, the whole set's cost in log units from the crosshair when it goes first. */
  from: number[];
  /** The index of the target that is best to kill first. */
  bestFirst: number;
}

/** The fastest order through the targets, and its predicted time in seconds. */
export interface FastestOrder {
  /** The targets in the order to kill them. */
  order: TrackPoint[];
  /** The predicted time to kill them all in that order, in seconds. */
  seconds: number;
}

/**
 * Over this many targets, only the ones cheapest to reach are ordered (the table doubles with each
 * one).
 */
const MAX_TARGETS = 14;

/**
 * How many sets' tables are kept: the overlay orders all the targets and then your kills among
 * them each frame, so one table would be made again twice a frame.
 */
const MAX_CACHED_TABLES = 4;

/**
 * Orders targets by Fitts' law. Costs are in log units (seconds = a per flick + b per unit). The
 * tables for the last few sets of targets are kept, so each frame only adds the flick from the
 * crosshair.
 */
export class OrderSolver {
  /**
   * The tables of the sets solved last, by their sorted ids joined with commas; the least recently
   * used first.
   */
  private readonly cache = new Map<string, CachedTable>();

  /** A solver with the run's fit. */
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

  /**
   * The best orders through the targets (only the MAX_TARGETS cheapest to reach, when there are
   * more); null without targets. The table is made again only when the set of targets changes.
   */
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
    const { ids, table } = this.cachedTable(key, candidates);
    const byId = new Map(candidates.map((target) => [target[0], target]));
    const inTableOrder = ids.map((id) => byId.get(id) as TrackPoint);
    const from = inTableOrder.map(
      (target, j) =>
        this.costFromCrosshair(target) + table.best[table.fullMask * table.targetCount + j],
    );
    return { targets: inTableOrder, table, from, bestFirst: from.indexOf(Math.min(...from)) };
  }

  /**
   * The fastest order through the targets from the crosshair, and its time; null without targets.
   */
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

  /** The kept table for a set of targets (made when it is not kept), now the most recently used. */
  private cachedTable(key: string, candidates: TrackPoint[]): CachedTable {
    const kept = this.cache.get(key);
    this.cache.delete(key);
    const cached = kept ?? {
      ids: candidates.map((target) => target[0]),
      table: this.table(candidates),
    };
    this.cache.set(key, cached);
    if (this.cache.size > MAX_CACHED_TABLES)
      this.cache.delete(this.cache.keys().next().value as string);
    return cached;
  }

  /**
   * The table of best paths through the targets (Held-Karp's dynamic programming over subsets):
   * each mask builds on the smaller masks before it. The table doubles with each target.
   */
  private table(targets: TrackPoint[]): OrderTable {
    const count = targets.length;
    const fullMask = (1 << count) - 1;
    const best = new Float64Array((fullMask + 1) * count).fill(Infinity);
    const next = new Int8Array((fullMask + 1) * count).fill(-1);
    const costs = new Float64Array(count * count);
    for (let j = 0; j < count; j++)
      for (let after = 0; after < count; after++)
        costs[j * count + after] = this.cost(targets[j], targets[after]);
    for (let j = 0; j < count; j++) best[(1 << j) * count + j] = 0;
    for (let mask = 1; mask <= fullMask; mask++) {
      for (let j = 0; j < count; j++) {
        if (!(mask & (1 << j)) || mask === 1 << j) continue;
        const rest = mask ^ (1 << j);
        for (let after = 0; after < count; after++) {
          if (!(rest & (1 << after))) continue;
          const cost = costs[j * count + after] + best[rest * count + after];
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
