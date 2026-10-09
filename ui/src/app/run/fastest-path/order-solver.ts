/**
 * The fastest order to kill the targets on screen, by Fitts' law fitted to the run's own flicks.
 * In: the clicking report's flicks and target radius, and each frame's targets (places and box
 * sizes) from the tracks. Out: the costs and orders path-analysis.ts works out the Pathing check and
 * overlays from.
 */

import { Flick, TargetSize, TrackFrame, TrackPoint } from '../../api';

/** Fitts' law's line: a flick takes a + b * log2(1 + D / W) seconds, D its distance, W the width. */
export interface FittsLine {
  /** The fixed time a flick takes, in seconds (the fit's intercept): the same in every direction. */
  a: number;
  /** The seconds each log unit of difficulty adds (the fit's slope). */
  b: number;
  /** The targets' width W, in degrees: twice the report's target radius, for a target with no box. */
  widthDeg: number;
}

/**
 * Fitts' law fitted to a run's kills: a flick takes a + k * b * log2(1 + D / W) seconds, D its
 * distance, W the target's width along the flick, and k the flick's direction's factor.
 */
export interface Fitts extends FittsLine {
  /**
   * The aiming time's factor in each of DIRECTION_SECTORS directions (sector 0 centered on right,
   * then counterclockwise): how much slower (over 1) or faster (under 1) the run's flicks that way
   * were than the fit says; 1 where too few flicks went that way.
   */
  directionFactors: number[];
}

/** Each target's box size by track id (width and height in degrees), from the frame's `wh`. */
export type TargetSizes = ReadonlyMap<number, TargetSize>;

/** A frame's targets' box sizes by track id; empty where the detector model gave no boxes. */
export function sizesOf(frame: TrackFrame | undefined): TargetSizes {
  const boxes = frame?.wh;
  return new Map(boxes ? frame.t.map((target, i) => [target[0], boxes[i]]) : []);
}

/** Fewer kills than this give no fit: a flat 0.1 s per unit is used. */
const MIN_FIT = 3;
/** The slope used without a fit, or when the fit's slope is not above 0, in seconds a log unit. */
const DEFAULT_B = 0.1;
/** The directions a flick's factor is fitted in: 8 sectors of 45 degrees. */
export const DIRECTION_SECTORS = 8;
/** A direction needs this many flicks for a factor of its own (else 1). */
const MIN_SECTOR_FLICKS = 5;
/** A flick whose fitted aiming time is under this many seconds says nothing of its direction. */
const MIN_AIMING_S = 0.02;
/** The least a direction's factor can be, so a few odd flicks cannot dominate a direction. */
const MIN_FACTOR = 0.5;
/** The most a direction's factor can be. */
const MAX_FACTOR = 2;
/** No direction factors: every direction as the fit says. */
const EVEN_DIRECTIONS: readonly number[] = Array<number>(DIRECTION_SECTORS).fill(1);

/** A flick that can be fitted: its total time in seconds and its log units at the run's width. */
interface FitSample {
  /** log2(1 + D0 / W). */
  units: number;
  /** Its total time, in seconds. */
  seconds: number;
  /** The direction to its target, in degrees (0 = right, 90 = up). */
  directionDeg: number;
}

/**
 * Fits Fitts' law by least squares to the flicks with a time and a start distance above 0: their
 * total time in seconds against log2(1 + D0 / W); then each direction's factor on the aiming part
 * (`directionFactors`). `radius` is the targets' radius in degrees.
 */
export function fitFitts(flicks: Flick[], radius: number): Fitts {
  const widthDeg = 2 * radius;
  const samples: FitSample[] = flicks
    .filter((kill) => kill.total != null && kill.D0 > 0)
    .map((kill) => ({
      units: Math.log2(1 + kill.D0 / widthDeg),
      seconds: kill.total as number,
      directionDeg: kill.direction_deg,
    }));
  const count = samples.length;
  if (count < MIN_FIT)
    return { a: 0, b: DEFAULT_B, widthDeg, directionFactors: [...EVEN_DIRECTIONS] };
  const meanUnits = samples.reduce((sum, sample) => sum + sample.units, 0) / count;
  const meanSeconds = samples.reduce((sum, sample) => sum + sample.seconds, 0) / count;
  const sumProducts = samples.reduce(
    (sum, sample) => sum + (sample.units - meanUnits) * (sample.seconds - meanSeconds),
    0,
  );
  const sumSquares = samples.reduce((sum, sample) => sum + (sample.units - meanUnits) ** 2, 0);
  const b = sumSquares > 0 && sumProducts > 0 ? sumProducts / sumSquares : DEFAULT_B;
  const a = meanSeconds - b * meanUnits;
  return { a, b, widthDeg, directionFactors: directionFactors(samples, a, b) };
}

/**
 * Each direction's factor on the aiming time: the median, over the direction's flicks, of the time
 * they took past the fixed part against the time the fit gives them (`seconds - a` over `b * units`),
 * within MIN_FACTOR and MAX_FACTOR; 1 for a direction with fewer than MIN_SECTOR_FLICKS such flicks. The median,
 * not the mean, so a flick that waited on a spawn does not decide its direction.
 */
function directionFactors(samples: FitSample[], a: number, b: number): number[] {
  const ratios: number[][] = Array.from({ length: DIRECTION_SECTORS }, () => []);
  for (const sample of samples) {
    const aiming = b * sample.units;
    if (aiming < MIN_AIMING_S || !Number.isFinite(sample.directionDeg)) continue;
    ratios[sectorOf(sample.directionDeg)].push((sample.seconds - a) / aiming);
  }
  return ratios.map((sector) =>
    sector.length < MIN_SECTOR_FLICKS
      ? 1
      : Math.min(MAX_FACTOR, Math.max(MIN_FACTOR, median(sector))),
  );
}

/** The sector (0 to DIRECTION_SECTORS - 1) a direction in degrees falls in, sector 0 centered on 0. */
export function sectorOf(directionDeg: number): number {
  const sectorDeg = 360 / DIRECTION_SECTORS;
  const turned = (((directionDeg + sectorDeg / 2) % 360) + 360) % 360;
  return Math.floor(turned / sectorDeg) % DIRECTION_SECTORS;
}

/** The median of some numbers (at least one). */
function median(values: number[]): number {
  const sorted = [...values].sort((x, y) => x - y);
  const middle = sorted.length >> 1;
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
}

/**
 * A target's width along a flick's line, in degrees: its box taken as an ellipse (a sphere's box is
 * its circle) cut through its center along the flick's direction (dx, dy, any length). `fallback`
 * for a target with no box or no flick.
 */
export function widthAlong(
  size: TargetSize | undefined,
  dx: number,
  dy: number,
  fallback: number,
): number {
  const length = Math.hypot(dx, dy);
  if (!size || size[0] <= 0 || size[1] <= 0 || length === 0) return fallback;
  const [cos, sin] = [dx / length, dy / length];
  return 1 / Math.hypot(cos / size[0], sin / size[1]);
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
   * The least time in seconds to visit a mask's targets starting at j; Infinity where j is not in
   * the mask.
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
  /** For each target, the whole set's time in seconds from the crosshair when it goes first. */
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
 * Orders targets by Fitts' law. Each flick's cost is its predicted time in seconds: a, plus b times
 * its log units at the target's own width along the flick, times its direction's factor. The tables
 * for the last few sets of targets are kept (by their ids: a set's places and sizes are taken as
 * the frame that made its table had them), so each frame only adds the flick from the crosshair.
 */
export class OrderSolver {
  /**
   * The tables of the sets solved last, by their sorted ids joined with commas; the least recently
   * used first.
   */
  private readonly cache = new Map<string, CachedTable>();

  /** A solver with the run's fit. */
  constructor(readonly fitts: Fitts) {}

  /** A flick's predicted time in seconds: dx, dy (degrees) from where it starts to the target. */
  flickSeconds(dx: number, dy: number, size: TargetSize | undefined): number {
    const { a, b, widthDeg, directionFactors } = this.fitts;
    const width = widthAlong(size, dx, dy, widthDeg);
    const factor = directionFactors[sectorOf((Math.atan2(dy, dx) * 180) / Math.PI)];
    return a + factor * b * Math.log2(1 + Math.hypot(dx, dy) / width);
  }

  /** From the crosshair to a target, in seconds; `sizes` gives its box (the run's width without). */
  costFromCrosshair(target: TrackPoint, sizes?: TargetSizes): number {
    return this.flickSeconds(target[1], target[2], sizes?.get(target[0]));
  }

  /** From one target to another, in seconds; `sizes` gives the second's box. */
  cost(from: TrackPoint, to: TrackPoint, sizes?: TargetSizes): number {
    return this.flickSeconds(to[1] - from[1], to[2] - from[2], sizes?.get(to[0]));
  }

  /** The predicted time in seconds of a path from the crosshair through the targets in order. */
  pathSeconds(order: TrackPoint[], sizes?: TargetSizes): number {
    return order.reduce(
      (seconds, target, i) =>
        seconds +
        (i ? this.cost(order[i - 1], target, sizes) : this.costFromCrosshair(target, sizes)),
      0,
    );
  }

  /**
   * The best orders through the targets (only the MAX_TARGETS cheapest to reach, when there are
   * more); null without targets. `sizes` gives each target's box. The table is made again only
   * when the set of targets changes.
   */
  solve(targets: TrackPoint[], sizes?: TargetSizes): Solution | null {
    if (!targets.length) return null;
    const candidates =
      targets.length > MAX_TARGETS
        ? [...targets]
            .sort((a, b) => this.costFromCrosshair(a, sizes) - this.costFromCrosshair(b, sizes))
            .slice(0, MAX_TARGETS)
        : targets;
    const key = candidates
      .map((target) => target[0])
      .sort((a, b) => a - b)
      .join(',');
    const { ids, table } = this.cachedTable(key, candidates, sizes);
    const byId = new Map(candidates.map((target) => [target[0], target]));
    const inTableOrder = ids.map((id) => byId.get(id) as TrackPoint);
    const from = inTableOrder.map(
      (target, j) =>
        this.costFromCrosshair(target, sizes) + table.best[table.fullMask * table.targetCount + j],
    );
    return { targets: inTableOrder, table, from, bestFirst: from.indexOf(Math.min(...from)) };
  }

  /**
   * The fastest order through the targets from the crosshair, and its time; null without targets.
   * `sizes` gives each target's box.
   */
  fastestOrder(targets: TrackPoint[], sizes?: TargetSizes): FastestOrder | null {
    const solution = this.solve(targets, sizes);
    if (!solution) return null;
    const { targets: inTableOrder, table } = solution;
    const order: TrackPoint[] = [];
    for (let mask = table.fullMask, j = solution.bestFirst; j >= 0;) {
      order.push(inTableOrder[j]);
      const after = table.next[mask * table.targetCount + j];
      mask ^= 1 << j;
      j = after;
    }
    return { order, seconds: solution.from[solution.bestFirst] };
  }

  /** The kept table for a set of targets (made when it is not kept), now the most recently used. */
  private cachedTable(key: string, candidates: TrackPoint[], sizes?: TargetSizes): CachedTable {
    const kept = this.cache.get(key);
    this.cache.delete(key);
    const cached = kept ?? {
      ids: candidates.map((target) => target[0]),
      table: this.table(candidates, sizes),
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
  private table(targets: TrackPoint[], sizes?: TargetSizes): OrderTable {
    const count = targets.length;
    const fullMask = (1 << count) - 1;
    const best = new Float64Array((fullMask + 1) * count).fill(Infinity);
    const next = new Int8Array((fullMask + 1) * count).fill(-1);
    const costs = new Float64Array(count * count);
    for (let j = 0; j < count; j++)
      for (let after = 0; after < count; after++)
        costs[j * count + after] = this.cost(targets[j], targets[after], sizes);
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
