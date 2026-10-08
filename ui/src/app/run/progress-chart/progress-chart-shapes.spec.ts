import {
  circlesPath,
  DOT_RADIUS,
  linePath,
  PICK_REACH,
  RING_RADIUS,
} from './progress-chart-shapes';

describe('progress chart shapes', () => {
  it('draws each point as a circle of two arcs, starting at its left edge', () => {
    expect(circlesPath([{ x: 10, y: 20 }], 2)).toBe('M8 20a2 2 0 1 0 4 0a2 2 0 1 0 -4 0');
    expect(circlesPath([], 2)).toBe('');
    expect(
      circlesPath(
        [
          { x: 1, y: 1 },
          { x: 5, y: 5 },
        ],
        1,
      ).match(/M/g),
    ).toHaveLength(2);
  });

  it('draws the median through its points in order, to a tenth of a pixel', () => {
    expect(
      linePath([
        { x: 0, y: 1.234 },
        { x: 10.06, y: 2 },
      ]),
    ).toBe('M0 1.2L10.1 2');
  });

  it("takes the dots' sizes from the tokens", () => {
    expect([DOT_RADIUS, RING_RADIUS, PICK_REACH]).toEqual([2.5, 7, 8]);
  });
});
