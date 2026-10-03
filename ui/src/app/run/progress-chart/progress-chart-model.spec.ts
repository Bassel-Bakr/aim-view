import { PastRun } from '../../platform/score-history';
import { dotNear, ordinal, progressChart } from './progress-chart-model';

const run = (stamp: string, score: number): PastRun => ({
  stamp,
  score,
  kills: null,
  accuracy: null,
});

const RUNS: PastRun[] = [
  run('2026.09.01-10.00.00', 500),
  run('2026.09.01-10.05.00', 520),
  run('2026.09.03-21.00.00', 610),
  run('2026.09.03-21.04.00', 580),
  run('2026.09.03-21.08.00', 600),
];
const SIZE = { width: 600, height: 200 };

describe('progressChart', () => {
  it('gives each day played the same width, its runs spread across it in order', () => {
    const m = progressChart(RUNS, null, SIZE);
    expect(m).not.toBeNull();
    const xs = m?.dots.map((d) => d.x) ?? [];
    const slot = (m!.right - m!.left) / 2;
    expect(xs[0]).toBeCloseTo(m!.left + slot * 0.25);
    expect(xs[1]).toBeCloseTo(m!.left + slot * 0.75);
    expect(xs[2]).toBeCloseTo(m!.left + slot * (1 + 1 / 6));
    expect(m?.dates.map((d) => d.label)).toEqual(['1 Sep 2026', '3 Sep 2026']);
  });

  it('marks the personal best and the run on the page, with its place', () => {
    // the recording's time is a second after its stats file's
    const m = progressChart(RUNS, '2026.09.03-21.04.01', SIZE);
    expect(m?.best.run.score).toBe(610);
    expect(m?.current?.run.score).toBe(580);
    expect(m?.rank).toBe(3);
    expect(m?.summary).toBe(
      '5 runs since 1 Sep 2026 · best 610 on 3 Sep 2026 · this run 580: 3rd best of 5',
    );
    expect(m?.dots.every((d) => d.y >= m.top && d.y <= m.bottom)).toBe(true);
    // a higher score is higher up
    expect(m!.best.y).toBeLessThan(m!.dots[0].y);
  });

  it('says so when the run on the page is not among them', () => {
    expect(progressChart(RUNS, '2026.09.03-22.00.00', SIZE)?.summary).toMatch(
      /this run is not among them$/,
    );
    expect(progressChart([], null, SIZE)).toBeNull();
  });

  it('draws the median of the runs around each one', () => {
    const m = progressChart(RUNS, null, SIZE)!;
    // every run is within four of the others: each point is the median of all five, 580
    const y580 = m.dots[3].y;
    for (const p of m.median) expect(p.y).toBeCloseTo(y580);
  });

  it('finds the dot under the pointer', () => {
    const m = progressChart(RUNS, null, SIZE)!;
    const d = m.dots[2];
    expect(dotNear(m, d.x + 2, d.y - 2, 6)).toBe(d);
    expect(dotNear(m, d.x + 40, d.y, 6)).toBeNull();
  });

  it('names places', () => {
    expect([1, 2, 3, 4, 11, 12, 13, 21, 22, 101].map(ordinal)).toEqual([
      '1st',
      '2nd',
      '3rd',
      '4th',
      '11th',
      '12th',
      '13th',
      '21st',
      '22nd',
      '101st',
    ]);
  });
});
