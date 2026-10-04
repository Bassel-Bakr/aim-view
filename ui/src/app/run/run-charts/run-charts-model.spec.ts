import { ClickReport, Flick, PathPoint } from '../../api';
import {
  BOX,
  clickGroup,
  directionWheel,
  fitFlickTimes,
  fittsChart,
  flickSpeeds,
  flickTimes,
  killShares,
  killTimes,
  landings,
  niceStep,
  pace,
  sector,
} from './run-charts-model';
import { fingerprint } from '../recording-context';

const flick = (killNumber: number, more: Partial<Flick>): Flick =>
  ({
    kill_number: killNumber,
    D0: 10,
    direction_deg: 0,
    total: 0.5,
    react: 0.1,
    flick: 0.2,
    shots: 1,
    parts: [0.1, 0.2, 0.1, 0.05, 0.05],
    click_off_xy: [0, 0],
    start_frame: 100 * killNumber,
    kill_frame: 100 * killNumber + 30,
    ...more,
  }) as Flick;

const report = (flicks: Flick[], paths: Record<string, PathPoint[]> = {}): ClickReport =>
  ({ mode: 'click', fps: 60, flicks, paths, summary: { radius: 0.5 } }) as unknown as ClickReport;

describe('niceStep', () => {
  it('rounds the axes to steps of 1, 2 or 5', () => {
    expect(niceStep(1)).toBe(0.2);
    expect(niceStep(0.6)).toBe(0.1);
    expect(niceStep(37)).toBe(5);
    expect(niceStep(480)).toBe(100);
  });
});

describe('killTimes', () => {
  it("stacks each kill's parts from the bottom, a kill without parts in one bar", () => {
    const model = killTimes(report([flick(1, {}), flick(2, { total: 0.8, parts: undefined })]));
    const [a, b] = model.bars;
    expect(a.segments).toHaveLength(5);
    // each part sits on the one before it, and the bar reaches the kill's time
    for (let i = 1; i < 5; i++) {
      expect(a.segments[i].y + a.segments[i].height).toBeCloseTo(a.segments[i - 1].y, 6);
    }
    expect(a.segments[4].y).toBeCloseTo(a.top, 6);
    expect(b.segments).toHaveLength(1);
    expect(b.top).toBeLessThan(a.top);
    expect(model.median?.label).toBe('median 650 ms');
  });
});

describe('fittsChart', () => {
  it("puts farther kills to the right and slower ones higher, and draws the run's curve", () => {
    const model = fittsChart(
      report([
        flick(1, { D0: 5, total: 0.4 }),
        flick(2, { D0: 20, total: 0.6 }),
        flick(3, { D0: 30, total: 0.9 }),
      ]),
    );
    expect(model.dots[1].x).toBeGreaterThan(model.dots[0].x);
    expect(model.dots[2].y).toBeLessThan(model.dots[1].y);
    expect(model.curve.startsWith('M')).toBe(true);
  });
});

describe('clickGroup', () => {
  it('turns every click so its flick comes from the left: past the center is to the right', () => {
    // a flick toward +x; the target ends 0.2 degrees to the crosshair's left, so the crosshair is past its center
    const past = flick(1, { direction_deg: 0, click_off_xy: [-0.2, 0] });
    // a flick toward +y, the crosshair 0.2 degrees short of the center
    const short = flick(2, { direction_deg: 90, click_off_xy: [0, 0.2], shots: 2 });
    const model = clickGroup(report([past, short]));
    expect(model.dots[0].x).toBeGreaterThan(model.center.x);
    expect(model.dots[0].y).toBeCloseTo(model.center.y, 6);
    expect(model.dots[1].x).toBeLessThan(model.center.x);
    expect(model.dots[1].missed).toBe(true);
    expect(model.dots[0].title).toContain('past');
    expect(model.dots[1].title).toContain('short of');
    expect(model.average?.x).toBeCloseTo(model.center.x, 6);
  });
});

describe('flickSpeeds', () => {
  it("lines every flick's speed up at the end of its main flick", () => {
    // the target closes in at 60 degrees a second until frame 118 (the main flick ends at 0.1 + 0.2 s = frame 118)
    const path: PathPoint[] = Array.from({ length: 30 }, (_unused, i) => [
      100 + i,
      Math.max(0, 18 - i),
      0,
    ]);
    const model = flickSpeeds(report([flick(1, {})], { '1': path }));
    expect(model.lines).toHaveLength(1);
    expect(model.median.startsWith('M')).toBe(true);
    expect(model.zero).toBeGreaterThan(model.box.left);
  });
});

describe('killShares', () => {
  it("splits each kill's time into reaction, flick, micro and confirmation shares", () => {
    // parts [0.1, 0.2, 0.1, 0.05, 0.05]: reaction 20%, flick 40%, micro (0.1 + 0.05) 30%, confirmation 10%
    const model = killShares(report([flick(1, {}), flick(2, { parts: undefined }), flick(3, {})]));
    expect(model.bars.map((b) => b.flick.kill_number)).toEqual([1, 3]);
    const plot = BOX.height - BOX.top - BOX.bottom;
    const heights = model.bars[0].segments.map((segment) => segment.height / plot);
    [0.2, 0.4, 0.3, 0.1].forEach((share, i) => expect(heights[i]).toBeCloseTo(share, 9));
    // the last part reaches the top, whatever the kill's time
    expect(model.bars[0].segments[3].y).toBeCloseTo(BOX.top, 9);
    expect(model.legend.map((step) => `${step.label} ${step.share}`)).toEqual([
      'Reaction 20%',
      'Flick 40%',
      'Micro 30%',
      'Confirmation 10%',
    ]);
    expect(model.bars[0].title).toContain('Micro 30% (150 ms)');
  });
});

describe('landings', () => {
  it('puts underflicks left of the center, overflicks right, stacked in their columns', () => {
    // radius 0.5: 0.95 and 0.93 degrees short are underflicks, 0.75 past an overflick, the others on the target
    const model = landings(
      report([
        flick(1, { end_left: 0.95 }),
        flick(2, { end_left: 0.15 }),
        flick(3, { end_left: -0.75 }),
        flick(4, { end_left: -0.25 }),
        flick(5, { end_left: 0.93 }),
      ]),
    );
    expect(model.cells.map((cell) => cell.landing)).toEqual(['under', 'on', 'over', 'on', 'under']);
    expect(model.cells[0].x).toBeLessThan(model.target.from);
    expect(model.cells[2].x).toBeGreaterThan(model.target.to);
    expect(model.target.from).toBeLessThan(model.zero);
    // the two underflicks share a column, the later one on top
    expect(model.cells[4].x).toBe(model.cells[0].x);
    expect(model.cells[4].y).toBeLessThan(model.cells[0].y);
    // landings -0.95, -0.15, 0.75, 0.25, -0.93: the median is -0.15
    expect(model.median?.label).toBe('median -0.15°');
    expect(model.cells[0].title).toBe('Kill 1: Underflick, 0.95° short of the center');
    expect(model.cells[2].title).toBe('Kill 3: Overflick, 0.75° past the center');
  });
});

describe('fitFlickTimes and flickTimes', () => {
  it("fits Fitts' law to the flicks' times, not their TTKs", () => {
    // W = 1 (radius 0.5); distances 1, 3 and 7 give log2(1 + D) = 1, 2, 3, and flick times 0.1 + 0.05 of that
    const flicks = [
      flick(1, { D0: 1, flick: 0.15, total: 0.9 }),
      flick(2, { D0: 3, flick: 0.2, total: 0.4 }),
      flick(3, { D0: 7, flick: 0.25, total: 0.7 }),
      flick(4, { D0: 5, flick: null as unknown as number }),
    ];
    const fit = fitFlickTimes(flicks, 0.5);
    expect(fit?.a).toBeCloseTo(0.1, 9);
    expect(fit?.b).toBeCloseTo(0.05, 9);
    const model = flickTimes(report(flicks));
    expect(model.dots).toHaveLength(3);
    expect(model.fit).toBe('time = 100 ms + 50 ms × log2(1 + D / 1.00°)');
    expect(model.curve.startsWith('M')).toBe(true);
    expect(fitFlickTimes(flicks.slice(0, 2), 0.5)).toBeNull();
    expect(flickTimes(report(flicks.slice(0, 2))).curve).toBe('');
  });
});

describe('sector', () => {
  it('turns each direction into one of eight, 0 for right', () => {
    expect(sector(0)).toBe(0);
    expect(sector(22)).toBe(0);
    expect(sector(23)).toBe(1);
    expect(sector(90)).toBe(2);
    expect(sector(359)).toBe(0);
    expect(sector(-45)).toBe(7);
  });
});

describe('directionWheel', () => {
  it('compares each direction against the median speed for the distance, best and weakest marked', () => {
    // 8 degrees each: right at 80 °/s, up at 40, left at 60; the distance group's median is 60
    const toward = (dir: number, time: number, from: number) =>
      [0, 1, 2].map((i) =>
        flick(from + i, { D0: 8, end_left: 0, direction_deg: dir, flick: time }),
      );
    const model = directionWheel(
      report([...toward(0, 0.1, 1), ...toward(90, 0.2, 4), ...toward(180, 8 / 60, 7)]),
    );
    const [right, , up, , left, , down] = model.wedges;
    expect(right.speed).toBeCloseTo(4 / 3, 9);
    expect(up.speed).toBeCloseTo(2 / 3, 9);
    expect(left.speed).toBeCloseTo(1, 9);
    expect(right.mark).toBe('best');
    expect(up.mark).toBe('weakest');
    expect(left.mark).toBeNull();
    expect(model.best?.name).toBe('right');
    expect(down.path).toBe('');
    expect(down.value).toBe('too few flicks');
    // the right wedge reaches out to the right, past the ring
    expect(right.path).toMatch(/^M260,100L/);
    expect(right.value).toBe('1.33×');
  });
});

describe('pace', () => {
  it('counts the kills in each 10 s window and marks the best', () => {
    // a kill every second for 10 s (TTK 1 s), then one every 3 s up to 28 s (TTK 3 s); the first flick starts at 0
    const kills = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 13, 16, 19, 22, 25, 28];
    const flicks = kills.map((seconds, i) => {
      const total = i < 10 ? 1 : 3;
      return flick(i + 1, { total, kill_frame: 60 * seconds, start_frame: 60 * (seconds - total) });
    });
    const model = pace(report(flicks));
    // the best window is the first: (0, 10] holds 10 kills
    expect(model.best?.label).toBe('best 10 s: 10 kills');
    expect(model.best?.x).toBeCloseTo(BOX.left, 9);
    expect(model.best?.width).toBeCloseTo(((BOX.width - BOX.left - BOX.right) * 10) / 28, 9);
    // the run's rate: 16 kills from its first flick (0 s) to its last kill (28 s), per 10 s
    expect(model.average?.label).toBe('run 5.7');
    // windows from 0, each kill up to 10 s, 13 and 16 (each must end by the last kill, at 28 s)
    expect(model.line.split('L')).toHaveLength(13);
    expect(model.kills).toHaveLength(16);
  });
});

describe('run charts', () => {
  it('keeps every number of each chart (a digest of each model as JSON)', () => {
    const run = variedRun();
    const models = [
      killTimes(run),
      fittsChart(run),
      clickGroup(run),
      flickSpeeds(run),
      killShares(run),
      landings(run),
      fitFlickTimes(run.flicks, run.summary.radius),
      flickTimes(run),
      directionWheel(run),
      pace(run),
    ];
    expect(models.map((model) => fingerprint(JSON.stringify(model)))).toEqual([
      'a0f042f7',
      'c5c01500',
      '0e85a91b',
      '54d75f0d',
      'a5219478',
      '887b6b5a',
      '1c602b67',
      '2cc26bf5',
      'b5fdd956',
      'ee43c358',
    ]);
  });
});

/** A run of 40 kills with varied distances, times, directions, landings and parts, and a path for each. */
function variedRun(): ClickReport {
  const flicks = Array.from({ length: 40 }, (_unused, i) =>
    flick(i + 1, {
      D0: 2 + ((i * 7) % 23),
      direction_deg: (i * 47) % 360,
      total: 0.3 + ((i * 13) % 10) / 20,
      react: 0.1 + (i % 4) / 50,
      flick: i % 9 === 0 ? (null as unknown as number) : 0.1 + ((i * 3) % 7) / 40,
      shots: i % 5 === 0 ? 2 : 1,
      parts: i % 11 === 3 ? undefined : [0.1, 0.15 + (i % 3) / 20, 0.05, 0.03 * (i % 4), 0.04],
      click_off_xy: [((i % 7) - 3) / 10, ((i % 5) - 2) / 10],
      end_left: ((i % 9) - 4) / 4,
      start_frame: 40 + 45 * i,
      kill_frame: 70 + 45 * i,
    }),
  );
  const paths = Object.fromEntries(
    flicks.map((kill) => [
      String(kill.kill_number),
      Array.from({ length: 40 }, (_unused, j): PathPoint => [
        kill.start_frame + j,
        Math.max(0, kill.D0 - j * 0.9),
        (j % 3) / 10,
      ]),
    ]),
  );
  return report(flicks, paths);
}
