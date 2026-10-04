import { TargetSize, TrackFrame, TrackPoint, Tracks } from '../../api';
import { faintScores, tracksUnder, tracksWithout } from './faint-scores';
import { faintStrip } from './faint-strip';

/** A frame of tracks: each target's id and place in degrees, its area, and its box and score where given. */
function trackFrame(
  index: number,
  targets: TrackPoint[],
  areas: number[],
  sizes?: TargetSize[],
  scores?: number[],
): TrackFrame {
  // eslint-disable-next-line id-length -- the core names TrackFrame's fields (generated/track-frame.ts)
  return { i: index, shift: [0, 0], t: targets, a: areas, wh: sizes, s: scores };
}

/** Ten frames of two tracks: one scoring 0.9 away from the crosshair, one 0.4 near it (1 degree off). */
const FRAMES: TrackFrame[] = Array.from({ length: 10 }, (_unused, i) =>
  trackFrame(
    i,
    [
      [1, 5, 0],
      [2, 1, 0],
    ],
    [10, 10],
    [
      [1, 1],
      [1, 1],
    ],
    [0.9, 0.4],
  ),
);

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
    const scored = (score: number) => trackFrame(0, [[7, 3, 0]], [1], undefined, [score]);
    const sevens = faintScores(
      [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 0.05].map(scored),
      2,
    );
    expect(sevens.scores.get(7)).toBe(0.9);
    expect(faintScores([scored(0.5), scored(0.6)], 2).level).toBeNull();
    expect(faintScores([trackFrame(0, [[1, 3, 0]], [1])], 0).level).toBeNull();
  });

  it('leaves out the tracks under the cut, each target whole', () => {
    const gone = tracksUnder(faintScores(FRAMES, 0), 0.9 - 0.3);
    expect([...gone]).toEqual([2]);
    const tracks: Tracks = { fps: 60, frames: FRAMES };
    const kept = tracksWithout(tracks, gone);
    expect(kept.frames[0]).toEqual(trackFrame(0, [[1, 5, 0]], [10], [[1, 1]], [0.9]));
    expect(tracksWithout(tracks, new Set())).toBe(tracks);
  });

  it('places the tracks along the strip, the ones under the cut marked', () => {
    const strip = faintStrip(faintScores(FRAMES, 0), 0.6);
    expect(strip.dots.map((dot) => [dot.id, dot.out])).toEqual([
      [1, false],
      [2, true],
    ]);
    expect(strip.dots[0].x).toBeGreaterThan(strip.cutX);
    expect(strip.dots[1].x).toBeLessThan(strip.cutX);
  });
});
