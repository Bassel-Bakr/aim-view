import { ClickReport, Flick } from '../../api';
import { killLanes } from './kill-lanes-model';

/** Four kills at 60 fps over a 10 s run: quick, long, past the tallest bar, and at the very end. */
const REPORT = {
  mode: 'click',
  fps: 60,
  flicks: [
    { kill_number: 1, kill_frame: 60, total: 0.5 },
    { kill_number: 2, kill_frame: 300, total: 1.5 },
    { kill_number: 3, kill_frame: 450, total: 3 },
    { kill_number: 4, kill_frame: 600, total: 0.01 },
  ] as Flick[],
} as unknown as ClickReport;

describe('killLanes', () => {
  it('puts each kill at its moment, its bar as tall as its time, long ones in the attention tone', () => {
    const lanes = killLanes(REPORT, 10, null);
    expect(lanes.kills.map((kill) => kill.atPercent)).toEqual([10, 50, 75, 100]);
    expect(lanes.kills.map((kill) => kill.tone)).toEqual(['quiet', 'long', 'long', 'quiet']);
    const barsHeight = 92 - lanes.split - 1;
    expect(lanes.kills[0].barHeight).toBeCloseTo(barsHeight / 4);
    // past the tallest bar: cut at the lane's top; the quickest still shows
    expect(lanes.kills[2].barHeight).toBe(barsHeight);
    expect(lanes.kills[3].barHeight).toBe(1);
    for (const kill of lanes.kills) expect(kill.barTop + kill.barHeight).toBe(92);
    expect(lanes.markTop * 2 + lanes.markHeight).toBe(lanes.split);
  });

  it('draws the picked kill apart', () => {
    const lanes = killLanes(REPORT, 10, REPORT.flicks[1]);
    expect(lanes.kills.map((kill) => [kill.picked, kill.tone])).toEqual([
      [false, 'quiet'],
      [true, 'picked'],
      [false, 'long'],
      [false, 'quiet'],
    ]);
  });
});
