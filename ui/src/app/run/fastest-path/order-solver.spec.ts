import { Flick, TrackPoint } from '../../api';
import { fingerprint } from '../recording-context';
import { fitFitts, OrderSolver } from './order-solver';

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
    expect(fitFitts([], 0.5)).toEqual({ a: 0, b: 0.1, widthDeg: 1 });
  });
});

describe('OrderSolver', () => {
  const solver = new OrderSolver({ a: 0, b: 1, widthDeg: 1 });

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
    const units = solver.pathUnits(best?.order ?? []);
    for (const order of [ts, [...ts].reverse()]) {
      expect(units).toBeLessThanOrEqual(solver.pathUnits(order) + 1e-9);
    }
    expect(best?.seconds).toBeCloseTo(units);
  });

  it('orders nothing when no target is on screen', () => {
    expect(solver.fastestOrder([])).toBeNull();
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
        fitted.pathUnits(targets),
      ];
    });
    expect(
      [Object.values(fitted.fitts), results].map((value) => fingerprint(JSON.stringify(value))),
    ).toEqual(['29bc07d8', 'e18a1c27']);
  });
});

const VARIED_FLICKS = Array.from(
  { length: 12 },
  (_unused, i) => ({ D0: 1 + ((i * 7) % 13), total: 0.2 + ((i * 5) % 9) / 30 }) as Flick,
);

/** Targets spread around the crosshair, each with its id and place in degrees. */
function targetSet(count: number): TrackPoint[] {
  return Array.from({ length: count }, (_unused, i): TrackPoint => [
    i + 1,
    ((i * 37) % 19) - 9,
    ((i * 23) % 11) - 5,
  ]);
}
