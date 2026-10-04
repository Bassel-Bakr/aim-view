import { PastRun } from '../../platform/score-history';
import { fingerprint } from '../recording-context';
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
    const model = progressChart(RUNS, null, SIZE);
    expect(model).not.toBeNull();
    const xs = model?.dots.map((dot) => dot.x) ?? [];
    const slot = (model!.right - model!.left) / 2;
    expect(xs[0]).toBeCloseTo(model!.left + slot * 0.25);
    expect(xs[1]).toBeCloseTo(model!.left + slot * 0.75);
    expect(xs[2]).toBeCloseTo(model!.left + slot * (1 + 1 / 6));
    expect(model?.dates.map((date) => date.label)).toEqual(['1 Sep 2026', '3 Sep 2026']);
  });

  it('marks the personal best and the run on the page, with its place', () => {
    // the recording's time is a second after its stats file's
    const model = progressChart(RUNS, '2026.09.03-21.04.01', SIZE);
    expect(model?.best.run.score).toBe(610);
    expect(model?.current?.run.score).toBe(580);
    expect(model?.rank).toBe(3);
    expect(model?.summary).toBe(
      '5 runs since 1 Sep 2026 · best 610 on 3 Sep 2026 · this run 580: 3rd best of 5',
    );
    expect(model?.dots.every((dot) => dot.y >= model.top && dot.y <= model.bottom)).toBe(true);
    // a higher score is higher up
    expect(model!.best.y).toBeLessThan(model!.dots[0].y);
  });

  it('says so when the run on the page is not among them', () => {
    expect(progressChart(RUNS, '2026.09.03-22.00.00', SIZE)?.summary).toMatch(
      /this run is not among them$/,
    );
    expect(progressChart([], null, SIZE)).toBeNull();
  });

  it('draws the median of the runs around each one', () => {
    const model = progressChart(RUNS, null, SIZE)!;
    // every run is within four of the others: each point is the median of all five, 580
    const y580 = model.dots[3].y;
    for (const point of model.median) expect(point.y).toBeCloseTo(y580);
  });

  it('keeps every number of the chart (a digest of the model as JSON)', () => {
    const days = Array.from({ length: 40 }, (_unused, i) =>
      run(
        `2026.${i < 30 ? '09' : '10'}.${String(1 + (i % 30)).padStart(2, '0')}-12.00.00`,
        400 + ((i * 53) % 170),
      ),
    );
    const models = [
      progressChart(RUNS, '2026.09.03-21.04.01', SIZE),
      progressChart(days, '2026.10.05-12.00.03', { width: 420, height: 180 }),
      progressChart([run('2026.09.01-10.00.00', 750)], null, SIZE),
      progressChart([run('2026.09.01-10.00.00', 0), run('2026.09.01-10.01.00', 0)], null, SIZE),
    ];
    expect(models.map((model) => fingerprint(JSON.stringify(model)))).toEqual([
      '9c0eaaa2',
      '32fd74e0',
      '4f453761',
      'd158823d',
    ]);
  });
});

describe('dotNear', () => {
  it('finds the dot under the pointer', () => {
    const model = progressChart(RUNS, null, SIZE)!;
    const dot = model.dots[2];
    expect(dotNear(model, dot.x + 2, dot.y - 2, 6)).toBe(dot);
    expect(dotNear(model, dot.x + 40, dot.y, 6)).toBeNull();
  });
});

describe('ordinal', () => {
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
