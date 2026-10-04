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

const flick = (n: number, more: Partial<Flick>): Flick =>
  ({
    n,
    D0: 10,
    dir: 0,
    total: 0.5,
    react: 0.1,
    flick: 0.2,
    shots: 1,
    parts: [0.1, 0.2, 0.1, 0.05, 0.05],
    click_off_xy: [0, 0],
    start_frame: 100 * n,
    kill_frame: 100 * n + 30,
    ...more,
  }) as Flick;

const report = (flicks: Flick[], paths: Record<string, PathPoint[]> = {}): ClickReport =>
  ({ mode: 'click', fps: 60, flicks, paths, summary: { radius: 0.5 } }) as unknown as ClickReport;

describe('run charts', () => {
  it('rounds the axes to steps of 1, 2 or 5', () => {
    expect(niceStep(1)).toBe(0.2);
    expect(niceStep(0.6)).toBe(0.1);
    expect(niceStep(37)).toBe(5);
    expect(niceStep(480)).toBe(100);
  });

  it("stacks each kill's parts from the bottom, a kill without parts in one bar", () => {
    const m = killTimes(report([flick(1, {}), flick(2, { total: 0.8, parts: undefined })]));
    const [a, b] = m.bars;
    expect(a.segments).toHaveLength(5);
    // each part sits on the one before it, and the bar reaches the kill's time
    for (let i = 1; i < 5; i++) {
      expect(a.segments[i].y + a.segments[i].height).toBeCloseTo(a.segments[i - 1].y, 6);
    }
    expect(a.segments[4].y).toBeCloseTo(a.top, 6);
    expect(b.segments).toHaveLength(1);
    expect(b.top).toBeLessThan(a.top);
    expect(m.median?.label).toBe('median 650 ms');
  });

  it("puts farther kills to the right and slower ones higher, and draws the run's curve", () => {
    const m = fittsChart(
      report([
        flick(1, { D0: 5, total: 0.4 }),
        flick(2, { D0: 20, total: 0.6 }),
        flick(3, { D0: 30, total: 0.9 }),
      ]),
    );
    expect(m.dots[1].x).toBeGreaterThan(m.dots[0].x);
    expect(m.dots[2].y).toBeLessThan(m.dots[1].y);
    expect(m.curve.startsWith('M')).toBe(true);
  });

  it('turns every click so its flick comes from the left: past the center is to the right', () => {
    // a flick toward +x; the target ends 0.2 degrees to the crosshair's left, so the crosshair is past its center
    const past = flick(1, { dir: 0, click_off_xy: [-0.2, 0] });
    // a flick toward +y, the crosshair 0.2 degrees short of the center
    const short = flick(2, { dir: 90, click_off_xy: [0, 0.2], shots: 2 });
    const m = clickGroup(report([past, short]));
    expect(m.dots[0].x).toBeGreaterThan(m.center.x);
    expect(m.dots[0].y).toBeCloseTo(m.center.y, 6);
    expect(m.dots[1].x).toBeLessThan(m.center.x);
    expect(m.dots[1].missed).toBe(true);
    expect(m.dots[0].title).toContain('past');
    expect(m.dots[1].title).toContain('short of');
    expect(m.average?.x).toBeCloseTo(m.center.x, 6);
  });

  it("lines every flick's speed up at the end of its main flick", () => {
    // the target closes in at 60 degrees a second until frame 118 (the main flick ends at 0.1 + 0.2 s = frame 118)
    const path: PathPoint[] = Array.from({ length: 30 }, (_, i) => [
      100 + i,
      Math.max(0, 18 - i),
      0,
    ]);
    const m = flickSpeeds(report([flick(1, {})], { '1': path }));
    expect(m.lines).toHaveLength(1);
    expect(m.median.startsWith('M')).toBe(true);
    expect(m.zero).toBeGreaterThan(m.box.left);
  });

  it("splits each kill's time into reaction, flick, micro and confirmation shares", () => {
    // parts [0.1, 0.2, 0.1, 0.05, 0.05]: reaction 20%, flick 40%, micro (0.1 + 0.05) 30%, confirmation 10%
    const m = killShares(report([flick(1, {}), flick(2, { parts: undefined }), flick(3, {})]));
    expect(m.bars.map((b) => b.flick.n)).toEqual([1, 3]);
    const plot = BOX.height - BOX.top - BOX.bottom;
    const heights = m.bars[0].segments.map((s) => s.height / plot);
    [0.2, 0.4, 0.3, 0.1].forEach((share, i) => expect(heights[i]).toBeCloseTo(share, 9));
    // the last part reaches the top, whatever the kill's time
    expect(m.bars[0].segments[3].y).toBeCloseTo(BOX.top, 9);
    expect(m.legend.map((l) => `${l.label} ${l.share}`)).toEqual([
      'Reaction 20%',
      'Flick 40%',
      'Micro 30%',
      'Confirmation 10%',
    ]);
    expect(m.bars[0].title).toContain('Micro 30% (150 ms)');
  });

  it('puts underflicks left of the center, overflicks right, stacked in their columns', () => {
    // radius 0.5: 0.95 and 0.93 degrees short are underflicks, 0.75 past an overflick, the others on the target
    const m = landings(
      report([
        flick(1, { end_left: 0.95 }),
        flick(2, { end_left: 0.15 }),
        flick(3, { end_left: -0.75 }),
        flick(4, { end_left: -0.25 }),
        flick(5, { end_left: 0.93 }),
      ]),
    );
    expect(m.cells.map((c) => c.landing)).toEqual(['under', 'on', 'over', 'on', 'under']);
    expect(m.cells[0].x).toBeLessThan(m.target.from);
    expect(m.cells[2].x).toBeGreaterThan(m.target.to);
    expect(m.target.from).toBeLessThan(m.zero);
    // the two underflicks share a column, the later one on top
    expect(m.cells[4].x).toBe(m.cells[0].x);
    expect(m.cells[4].y).toBeLessThan(m.cells[0].y);
    // landings -0.95, -0.15, 0.75, 0.25, -0.93: the median is -0.15
    expect(m.median?.label).toBe('median -0.15°');
    expect(m.cells[0].title).toBe('Kill 1: Underflick, 0.95° short of the center');
    expect(m.cells[2].title).toBe('Kill 3: Overflick, 0.75° past the center');
  });

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
    const m = flickTimes(report(flicks));
    expect(m.dots).toHaveLength(3);
    expect(m.fit).toBe('time = 100 ms + 50 ms × log2(1 + D / 1.00°)');
    expect(m.curve.startsWith('M')).toBe(true);
    expect(fitFlickTimes(flicks.slice(0, 2), 0.5)).toBeNull();
    expect(flickTimes(report(flicks.slice(0, 2))).curve).toBe('');
  });

  it('turns each direction into one of eight, 0 for right', () => {
    expect(sector(0)).toBe(0);
    expect(sector(22)).toBe(0);
    expect(sector(23)).toBe(1);
    expect(sector(90)).toBe(2);
    expect(sector(359)).toBe(0);
    expect(sector(-45)).toBe(7);
  });

  it('compares each direction against the median speed for the distance, best and weakest marked', () => {
    // 8 degrees each: right at 80 °/s, up at 40, left at 60; the distance group's median is 60
    const toward = (dir: number, time: number, from: number) =>
      [0, 1, 2].map((i) => flick(from + i, { D0: 8, end_left: 0, dir, flick: time }));
    const m = directionWheel(
      report([...toward(0, 0.1, 1), ...toward(90, 0.2, 4), ...toward(180, 8 / 60, 7)]),
    );
    const [right, , up, , left, , down] = m.wedges;
    expect(right.speed).toBeCloseTo(4 / 3, 9);
    expect(up.speed).toBeCloseTo(2 / 3, 9);
    expect(left.speed).toBeCloseTo(1, 9);
    expect(right.mark).toBe('best');
    expect(up.mark).toBe('weakest');
    expect(left.mark).toBeNull();
    expect(m.best?.name).toBe('right');
    expect(down.path).toBe('');
    expect(down.value).toBe('too few flicks');
    // the right wedge reaches out to the right, past the ring
    expect(right.path).toMatch(/^M260,100L/);
    expect(right.value).toBe('1.33×');
  });

  it('counts the kills in each 10 s window and marks the best', () => {
    // a kill every second for 10 s (TTK 1 s), then one every 3 s up to 28 s (TTK 3 s); the first flick starts at 0
    const kills = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 13, 16, 19, 22, 25, 28];
    const flicks = kills.map((t, i) => {
      const total = i < 10 ? 1 : 3;
      return flick(i + 1, { total, kill_frame: 60 * t, start_frame: 60 * (t - total) });
    });
    const m = pace(report(flicks));
    // the best window is the first: (0, 10] holds 10 kills
    expect(m.best?.label).toBe('best 10 s: 10 kills');
    expect(m.best?.x).toBeCloseTo(BOX.left, 9);
    expect(m.best?.width).toBeCloseTo(((BOX.width - BOX.left - BOX.right) * 10) / 28, 9);
    // the run's rate: 16 kills from its first flick (0 s) to its last kill (28 s), per 10 s
    expect(m.average?.label).toBe('run 5.7');
    // windows from 0, each kill up to 10 s, 13 and 16 (each must end by the last kill, at 28 s)
    expect(m.line.split('L')).toHaveLength(13);
    expect(m.kills).toHaveLength(16);
  });
});
