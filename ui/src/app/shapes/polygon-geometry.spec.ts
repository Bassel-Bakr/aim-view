import { CropVertex, Shape } from '../api';
import { placedPolygon, polygonVertices, refitPolygon, sidesOf } from './polygon-geometry';

/** A polygon for the tests: a hexagon at (50, 50), 20 wide and 20 tall, unless overrides say otherwise. */
function polygon(overrides: Partial<Shape>): Shape {
  return {
    id: 'p',
    kind: 'polygon',
    box: [50, 50, 20, 20],
    angle: 0,
    face: null,
    solid: null,
    points: null,
    sides: 6,
    depth: 0,
    role: null,
    model: null,
    ...overrides,
  };
}

/** Points rounded to a millionth of a pixel, to compare. */
const rounded = (points: readonly number[][]) =>
  points.map((point) => point.map((value) => Math.round(value * 1e6) / 1e6 + 0));

describe('a polygon', () => {
  it('puts its vertices on the ellipse of its turned frame, the first at the top, clockwise', () => {
    expect(rounded(polygonVertices(polygon({ sides: 4 })))).toEqual([
      [50, 40],
      [60, 50],
      [50, 60],
      [40, 50],
    ]);
    const square = polygon({
      sides: 4,
      angle: 45,
      box: [50, 50, 20 * Math.SQRT2, 20 * Math.SQRT2],
    });
    expect(rounded(polygonVertices(square))).toEqual([
      [60, 40],
      [60, 60],
      [40, 60],
      [40, 40],
    ]);
    expect(polygonVertices(polygon({ sides: null }))).toHaveLength(6);
    expect(rounded(polygonVertices(polygon({ sides: 4, box: [50, 50, 40, 20] })))[1]).toEqual([
      70, 50,
    ]);
  });

  it('is placed vertex by vertex from where its vertices are, and made uniform again as it was', () => {
    const turned = polygon({ sides: 5, angle: 30, box: [40, 60, 24, 24] });
    const placed = placedPolygon(turned);
    expect(placed.points).toHaveLength(5);
    expect(sidesOf(placed)).toBe(5);
    expect(rounded(placed.points ?? [])).toEqual(rounded(polygonVertices(turned)));
    const back = refitPolygon(placed);
    expect(back.points).toBeNull();
    expect(rounded([back.box])).toEqual([[40, 60, 24, 24]]);
    expect(back.angle).toBeCloseTo(30, 6);
    expect(placedPolygon(placed)).toBe(placed);
    expect(refitPolygon(turned)).toBe(turned);
  });

  it('fits the nearest regular polygon to vertices moved on their own', () => {
    const placed = placedPolygon(polygon({ sides: 4 }));
    const points = (placed.points ?? []).map(([x, y], i): CropVertex => [x + (i === 1 ? 4 : 0), y]);
    const fitted = refitPolygon({ ...placed, points });
    expect(fitted.box[0]).toBeCloseTo(51, 6);
    expect(fitted.box[1]).toBeCloseTo(50, 6);
    expect(fitted.box[2]).toBeCloseTo(fitted.box[3], 6);
    expect(fitted.sides).toBe(4);
  });
});
