import { Flick, TargetSize, TrackPoint } from '../../api';
import { fingerprint } from '../recording-context';
import {
  DIRECTION_SECTORS,
  Fitts,
  fitFitts,
  OrderSolver,
  sectorOf,
  Solution,
  widthAlong,
} from './order-solver';

/** Every direction as the fit says. */
const EVEN = Array<number>(DIRECTION_SECTORS).fill(1);

describe('fitFitts', () => {
  it('fits time to the log of distance over width', () => {
    // t = 0.1 + 0.05 * log2(1 + D / 1): D = 1, 3, 7 give 1, 2, 3 units
    const flicks = [1, 3, 7].map(
      (distance, i) => ({ D0: distance, total: 0.1 + 0.05 * (i + 1) }) as Flick,
    );
    const fit = fitFitts(flicks, 0.5);
    expect(fit.widthDeg).toBe(1);
    expect(fit.a).toBeCloseTo(0.1);
    expect(fit.b).toBeCloseTo(0.05);
  });

  it('falls back to a flat rate with too few kills', () => {
    expect(fitFitts([], 0.5)).toEqual({ a: 0, b: 0.1, widthDeg: 1, directionFactors: EVEN });
  });

  it("fits a direction's factor from its flicks' time past the fit", () => {
    // 6 flicks right take 1.5 times the fitted aiming time, 6 up take it as fitted
    const flick = (directionDeg: number, units: number, slow: number) =>
      ({
        D0: 2 ** units - 1,
        total: 0.1 + slow * 0.05 * units,
        direction_deg: directionDeg,
      }) as Flick;
    const flicks = [1, 2, 3, 4, 5, 6].flatMap((units) => [
      flick(0, units, 1.5),
      flick(90, units, 1),
    ]);
    const fit = fitFitts(flicks, 0.5);
    const right = fit.directionFactors[sectorOf(0)];
    const up = fit.directionFactors[sectorOf(90)];
    expect(right / up).toBeCloseTo(1.5, 1);
    expect(fit.directionFactors[sectorOf(180)]).toBe(1);
  });
});

describe('sectorOf and widthAlong', () => {
  it('puts each direction in its 45 degree sector, right first and counterclockwise', () => {
    expect([0, 22, 23, 90, 180, -90, 359].map(sectorOf)).toEqual([0, 0, 1, 2, 4, 6, 0]);
  });

  it("takes a box's width along the flick: its width across, its height up", () => {
    expect(widthAlong([4, 2], 1, 0, 9)).toBeCloseTo(4);
    expect(widthAlong([4, 2], 0, 1, 9)).toBeCloseTo(2);
    expect(widthAlong([3, 3], 1, 1, 9)).toBeCloseTo(3);
    expect(widthAlong(undefined, 1, 0, 9)).toBe(9);
  });
});

describe('OrderSolver', () => {
  const solver = new OrderSolver({ a: 0, b: 1, widthDeg: 1, directionFactors: EVEN });

  it('takes the near target first when both lie the same way', () => {
    const near: TrackPoint = [1, 2, 0];
    const far: TrackPoint = [2, 8, 0];
    expect(solver.fastestOrder([far, near])?.order.map((target) => target[0])).toEqual([1, 2]);
  });

  it('predicts the order no slower than any other', () => {
    const ts: TrackPoint[] = [
      [1, 5, 5],
      [2, -6, 1],
      [3, 2, -7],
      [4, 9, 0],
    ];
    const best = solver.fastestOrder(ts);
    const seconds = solver.pathSeconds(best?.order ?? []);
    for (const order of [ts, [...ts].reverse()]) {
      expect(seconds).toBeLessThanOrEqual(solver.pathSeconds(order) + 1e-9);
    }
    expect(best?.seconds).toBeCloseTo(seconds);
  });

  it('with even directions and no boxes, finds the order the run-wide formula finds', () => {
    // n flicks take n * a + b * (the log units at the run's width), whatever the order
    const fitts: Fitts = { a: 0.15, b: 0.07, widthDeg: 0.8, directionFactors: EVEN };
    const even = new OrderSolver(fitts);
    const targets = targetSet(6);
    const runWide = (order: TrackPoint[]) =>
      order.reduce((seconds, target, i) => {
        const [x, y] = i ? [order[i - 1][1], order[i - 1][2]] : [0, 0];
        return (
          seconds +
          fitts.a +
          fitts.b * Math.log2(1 + Math.hypot(target[1] - x, target[2] - y) / 0.8)
        );
      }, 0);
    const least = Math.min(...permutations(targets).map(runWide));
    expect(even.fastestOrder(targets)?.seconds).toBeCloseTo(least, 9);
  });
});

describe("OrderSolver's widths and directions", () => {
  const solver = new OrderSolver({ a: 0, b: 1, widthDeg: 1, directionFactors: EVEN });

  it('takes the narrow target first while it is near, and leaves the long flick to the wide one', () => {
    // narrow first: log2(1 + 4 / 0.5) + log2(1 + 8 / 2) = 5.49; wide first: log2(1 + 4 / 2) +
    // log2(1 + 8 / 0.5) = 5.67, though both orders cover the same distance
    const narrow: TrackPoint = [1, -4, 0];
    const wide: TrackPoint = [2, 4, 0];
    const sizes = new Map<number, TargetSize>([
      [1, [0.5, 0.5]],
      [2, [2, 2]],
    ]);
    const fastest = solver.fastestOrder([narrow, wide], sizes);
    expect(fastest?.order.map((target) => target[0])).toEqual([1, 2]);
    expect(fastest?.seconds).toBeCloseTo(Math.log2(9) + Math.log2(5), 9);
  });

  it('goes the faster direction first, everything else equal', () => {
    const slowLeft = EVEN.map((factor, sector) => (sector === sectorOf(180) ? 1.5 : factor));
    const leftSlow = new OrderSolver({ a: 0.1, b: 0.1, widthDeg: 1, directionFactors: slowLeft });
    const left: TrackPoint = [1, -4, 0];
    const right: TrackPoint = [2, 4, 0];
    // right first (fast), then left 8 degrees (slow), against left first (slow), then right (fast)
    const order = leftSlow.fastestOrder([left, right])?.order.map((target) => target[0]);
    expect(leftSlow.pathSeconds([right, left])).not.toBe(leftSlow.pathSeconds([left, right]));
    expect(order).toEqual(
      leftSlow.pathSeconds([right, left]) < leftSlow.pathSeconds([left, right]) ? [2, 1] : [1, 2],
    );
  });
});

describe("OrderSolver's tables", () => {
  const solver = new OrderSolver({ a: 0, b: 1, widthDeg: 1, directionFactors: EVEN });

  it('orders nothing when no target is on screen', () => {
    expect(solver.fastestOrder([])).toBeNull();
  });

  it('orders the same with its kept tables as a new solver does', () => {
    const fitted = new OrderSolver(fitFitts(VARIED_FLICKS, 0.4));
    const sets = [targetSet(9), targetSet(5), targetSet(9).reverse(), targetSet(5)];
    for (const targets of sets) {
      const fresh = new OrderSolver(fitted.fitts);
      expect(fitted.fastestOrder(targets)).toEqual(fresh.fastestOrder(targets));
      expect(costById(fitted.solve(targets))).toEqual(costById(fresh.solve(targets)));
    }
  });

  it('reuses the table of a set after another set is solved', () => {
    const all = targetSet(9);
    const yours = all.slice(0, 4);
    const first = solver.solve(all)?.table;
    solver.solve(yours);
    expect(solver.solve([...all].reverse())?.table).toBe(first);
  });

  it('keeps its fits, orders and costs (a digest of them)', () => {
    const fitted = new OrderSolver(fitFitts(VARIED_FLICKS, 0.4));
    const results = [3, 9, 16, 9].map((count) => {
      const targets = targetSet(count);
      const solution = fitted.solve(targets);
      const fastest = fitted.fastestOrder(targets);
      return [
        solution?.from ?? null,
        solution?.targets.map((target) => target[0]) ?? null,
        fastest?.order.map((target) => target[0]) ?? null,
        fastest?.seconds ?? null,
        fitted.pathSeconds(targets),
      ];
    });
    expect(
      [Object.values(fitted.fitts), results].map((value) => fingerprint(JSON.stringify(value))),
    ).toEqual(['7d996b9a', '8e38689f']);
  });
});

const VARIED_FLICKS = Array.from(
  { length: 12 },
  (_unused, i) => ({ D0: 1 + ((i * 7) % 13), total: 0.2 + ((i * 5) % 9) / 30 }) as Flick,
);

/** Every order of some items. */
function permutations<T>(items: T[]): T[][] {
  if (items.length <= 1) return [items];
  return items.flatMap((item, i) =>
    permutations([...items.slice(0, i), ...items.slice(i + 1)]).map((rest) => [item, ...rest]),
  );
}

/** Each target's id and the set's cost when it goes first, by id (tables differ in their order). */
function costById(solution: Solution | null): number[][] {
  const pairs = solution?.targets.map((target, j) => [target[0], solution.from[j]]) ?? [];
  return pairs.sort((a, b) => a[0] - b[0]);
}

/** Targets spread around the crosshair, each with its id and place in degrees. */
function targetSet(count: number): TrackPoint[] {
  return Array.from({ length: count }, (_unused, i): TrackPoint => [
    i + 1,
    ((i * 37) % 19) - 9,
    ((i * 23) % 11) - 5,
  ]);
}
