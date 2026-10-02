import { PathPoint } from '../api';
import { smoothed, speeds } from './speed';

describe('speeds', () => {
  it('turns a path at a steady pace into that pace, in degrees per second', () => {
    const path: PathPoint[] = [0, 1, 2, 3, 4].map((f) => [f, -f * 0.5, 0]);
    const v = speeds(path, 100, false);
    expect(v[0]).toEqual([0, 0]);
    for (const [, s] of v.slice(1)) expect(s).toBeCloseTo(50);
  });

  it('spreads a gap in the frames over the frames it spans', () => {
    const path: PathPoint[] = [
      [0, 0, 0],
      [2, 1, 0],
    ];
    expect(speeds(path, 100, false)[1][1]).toBeCloseTo(50);
  });
});

describe('smoothed', () => {
  it('leaves a steady speed as it is and ignores the first placeholder', () => {
    const out = smoothed(
      [
        [0, 0],
        [1, 40],
        [2, 40],
        [3, 40],
      ],
      1,
    );
    for (const [, s] of out) expect(s).toBeCloseTo(40);
  });
});
