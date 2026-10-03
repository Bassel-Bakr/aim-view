import { splitRuns } from './split-runs';

/** n frames at 60 a second from 0, a key frame every `every` frames. */
function video(n: number, every: number) {
  const times = Array.from({ length: n }, (_, i) => i / 60);
  return { times, keys: times.filter((_, i) => i % every === 0) };
}

describe('splitRuns', () => {
  it('cuts at the key frame nearest the middle, and the runs cover every frame once', () => {
    const { times, keys } = video(6038, 240);
    const runs = splitRuns(times, keys, 2, 600);
    expect(runs).toEqual([
      { from: 0, to: times[3120], first: 0, frames: 3120 },
      { from: times[3120], to: null, first: 3120, frames: 2918 },
    ]);
  });

  it('keeps a short recording in one run', () => {
    const { times, keys } = video(900, 240);
    expect(splitRuns(times, keys, 2, 600)).toEqual([{ from: 0, to: null, first: 0, frames: 900 }]);
  });

  it('keeps a recording with one key frame in one run', () => {
    const { times, keys } = video(6000, 100000);
    expect(splitRuns(times, keys, 2, 600)).toEqual([{ from: 0, to: null, first: 0, frames: 6000 }]);
  });

  it('makes no cut that leaves a run too short', () => {
    // the only later key frame is 100 frames from the end
    const { times } = video(6000, 1);
    expect(splitRuns(times, [0, times[5900]], 2, 600)).toHaveLength(1);
  });

  it('splits into more runs, each from a key frame', () => {
    const { times, keys } = video(9000, 300);
    const runs = splitRuns(times, keys, 3, 600);
    expect(runs.map((r) => r.first)).toEqual([0, 3000, 6000]);
    expect(runs.reduce((a, r) => a + r.frames, 0)).toBe(9000);
    expect(runs.slice(1).every((r) => keys.includes(r.from))).toBe(true);
  });
});
