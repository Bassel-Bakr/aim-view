import { Timeline, TrackState } from '../track';
import { timelineChart } from './timeline-model';

/** A run of the given length: on, off and switching in turn, with frames where no bot was seen. */
function run(frameCount: number, fps: number): Timeline {
  const state = new Int8Array(frameCount);
  const outsideDeg = new Float32Array(frameCount);
  for (let i = 0; i < frameCount; i++) {
    state[i] = [
      TrackState.On,
      TrackState.Off,
      TrackState.Off,
      TrackState.Switching,
      TrackState.NoBot,
    ][Math.floor(i / 7) % 5];
    outsideDeg[i] =
      state[i] === TrackState.Off ? (i % 13) / 4 : state[i] === TrackState.On ? 0 : NaN;
  }
  return {
    start: 12,
    frameCount,
    fps,
    state,
    outsideDeg,
    capDeg: 2.5,
    deaths: [40, frameCount - 30],
  };
}

/** The x of every column a path's rectangles start at (`M<x> ...`). */
function starts(path: string): number[] {
  return [...path.matchAll(/M(\d+) /g)].map((match) => Number(match[1]));
}

describe('timelineChart', () => {
  it('lays the strip out per pixel column, one rectangle per run of one state', () => {
    // 35 frames over 35 columns: each column is one frame, each state 7 columns in a row
    const chart = timelineChart(run(35, 60), 35);
    expect(starts(chart.strip.on)).toEqual([0]);
    expect(starts(chart.strip.off)).toEqual([7]);
    expect(starts(chart.strip.switching)).toEqual([21]);
    expect(starts(chart.strip.noBot)).toEqual([28]);
    expect(chart.strip.off).toContain('H21');
  });

  it('draws the areas, the grid, the deaths and the scale inside the chart', () => {
    const chart = timelineChart(run(600, 60), 300);
    expect(chart.width).toBe(300);
    expect(chart.grid.map((line) => line.dashed)).toEqual([false, true, true]);
    const [bottom, , top] = chart.grid.map((line) => line.y);
    expect(top).toBeLessThan(bottom);
    for (const path of [chart.furthest, chart.mean]) {
      expect(path.startsWith(`M0 `)).toBe(true);
      expect(path.endsWith('Z')).toBe(true);
    }
    // two deaths: at frame 40 and 30 frames before the end
    expect(starts(chart.deaths)).toEqual([20, 285]);
    expect(chart.scale.map((label) => label.text)).toEqual(['2.5° off', '0°: on the bot']);
  });

  it('labels every ten seconds, every twenty on a long run, the last one kept whole', () => {
    expect(timelineChart(run(1800, 60), 600).seconds.map((label) => label.text)).toEqual([
      '0 s',
      '10 s',
      '20 s',
      '30 s',
    ]);
    const long = timelineChart(run(6000, 60), 600).seconds;
    expect(long.map((label) => label.text)).toEqual([
      '0 s',
      '20 s',
      '40 s',
      '60 s',
      '80 s',
      '100 s',
    ]);
    expect(long[0].anchor).toBe('start');
    expect(long.at(-1)?.anchor).toBe('end');
  });
});
