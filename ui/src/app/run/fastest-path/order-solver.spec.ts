import { Flick, TrackPoint } from '../../api';
import { fitFitts, OrderSolver } from './order-solver';

describe('fitFitts', () => {
  it('fits time to the log of distance over width', () => {
    // t = 0.1 + 0.05 * log2(1 + D / 1): D = 1, 3, 7 give 1, 2, 3 units
    const flicks = [1, 3, 7].map((D, i) => ({ D0: D, total: 0.1 + 0.05 * (i + 1) }) as Flick);
    const f = fitFitts(flicks, 0.5);
    expect(f.W).toBe(1);
    expect(f.a).toBeCloseTo(0.1);
    expect(f.b).toBeCloseTo(0.05);
  });

  it('falls back to a flat rate with too few kills', () => {
    expect(fitFitts([], 0.5)).toEqual({ a: 0, b: 0.1, W: 1 });
  });
});

describe('OrderSolver', () => {
  const solver = new OrderSolver({ a: 0, b: 1, W: 1 });

  it('takes the near target first when both lie the same way', () => {
    const near: TrackPoint = [1, 2, 0];
    const far: TrackPoint = [2, 8, 0];
    expect(solver.fastestOrder([far, near])?.order.map((t) => t[0])).toEqual([1, 2]);
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
});
