import { ClickReport, Flick, PathPoint } from '../../api';
import { clickGroup, fittsChart, flickSpeeds, killTimes, niceStep } from './run-charts-model';

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
    const m = killTimes(report([flick(1, {}), flick(2, { total: 0.8, parts: null })]));
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
});
