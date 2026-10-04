import { PathPoint } from '../api';
import { smoothed, speeds } from './speed';

describe('speeds', () => {
  it('turns a path at a steady pace into that pace, in degrees per second', () => {
    const path: PathPoint[] = [0, 1, 2, 3, 4].map((frame) => [frame, -frame * 0.5, 0]);
    const points = speeds(path, 100, false);
    expect(points[0]).toEqual([0, 0]);
    for (const [, speed] of points.slice(1)) expect(speed).toBeCloseTo(50);
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
    for (const [, speed] of out) expect(speed).toBeCloseTo(40);
  });
});
