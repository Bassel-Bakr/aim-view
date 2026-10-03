import { TrackFrame, Tracks } from '../../api';
import { faintScores, tracksUnder, tracksWithout } from './faint-scores';
import { faintStrip } from './faint-strip';

/** Ten frames of two tracks: one scoring 0.9 away from the crosshair, one 0.4 near it (1 degree off). */
const FRAMES: TrackFrame[] = Array.from({ length: 10 }, (_, i) => ({
  i,
  t: [
    [1, 5, 0],
    [2, 1, 0],
  ],
  a: [10, 10],
  wh: [
    [1, 1],
    [1, 1],
  ],
  s: [0.9, 0.4],
}));

describe('faint scores', () => {
  it('scores each track and finds the level, as python/review.py does', () => {
    const near = faintScores(FRAMES, 2);
    expect([...near.scores]).toEqual([[1, 0.9]]);
    expect(near.level).toBe(0.9);
    // a tracking run counts the frames near the crosshair too
    const all = faintScores(FRAMES, 0);
    expect([...all.scores]).toEqual([
      [1, 0.9],
      [2, 0.4],
    ]);
    expect([...all.frames]).toEqual([
      [1, 10],
      [2, 10],
    ]);
    expect(all.level).toBe(0.9);
  });

  it('takes the 90th percentile of a track, and needs 3 frames', () => {
    const f = (s: number): TrackFrame => ({ i: 0, t: [[7, 3, 0]], s: [s] });
    const sc = faintScores([0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 0.05].map(f), 2);
    expect(sc.scores.get(7)).toBe(0.9);
    expect(faintScores([f(0.5), f(0.6)], 2).level).toBeNull();
    expect(faintScores([{ i: 0, t: [[1, 3, 0]] }], 0).level).toBeNull();
  });

  it('leaves out the tracks under the cut, each target whole', () => {
    const sc = faintScores(FRAMES, 0);
    const gone = tracksUnder(sc, 0.9 - 0.3);
    expect([...gone]).toEqual([2]);
    const tracks: Tracks = { fps: 60, frames: FRAMES };
    const kept = tracksWithout(tracks, gone);
    expect(kept.frames[0]).toEqual({ i: 0, t: [[1, 5, 0]], a: [10], wh: [[1, 1]], s: [0.9] });
    expect(tracksWithout(tracks, new Set())).toBe(tracks);
  });

  it('places the tracks along the strip, the ones under the cut marked', () => {
    const strip = faintStrip(faintScores(FRAMES, 0), 0.6);
    expect(strip.dots.map((d) => [d.id, d.out])).toEqual([
      [1, false],
      [2, true],
    ]);
    expect(strip.dots[0].x).toBeGreaterThan(strip.cutX);
    expect(strip.dots[1].x).toBeLessThan(strip.cutX);
  });
});
