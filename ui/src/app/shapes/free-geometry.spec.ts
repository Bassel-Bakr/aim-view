import { CropVertex, Shape } from '../api';
import { freeEdges, freePoints, freeSides, movedSide, movedVertex, onFree } from './free-geometry';
import { solidCorners, solidFaces } from './solid-geometry';

/** A box at (50, 50), 20 by 20, flat unless a solid is given. */
function box(overrides: Partial<Shape> = {}): Shape {
  return {
    id: 's',
    kind: 'box',
    box: [50, 50, 20, 20],
    angle: 0,
    face: null,
    solid: null,
    points: null,
    sides: null,
    depth: 0,
    role: null,
    model: null,
    ...overrides,
  };
}

const FLAT: CropVertex[] = [
  [40, 40],
  [60, 40],
  [60, 60],
  [40, 60],
];

describe('a box placed by its vertices', () => {
  it('moves one corner on its own, the box round them following', () => {
    const placed = movedVertex(box(), FLAT, 2, [70, 66]);
    expect(placed.points).toEqual([
      [40, 40],
      [60, 40],
      [70, 66],
      [40, 60],
    ]);
    expect(placed.box).toEqual([55, 53, 30, 26]);
    expect(onFree(freePoints(placed)!, [68, 63], 0)).toBe(true);
    expect(freePoints(box({ kind: 'pill', points: FLAT }))).toBeNull();
  });

  it('moves a side’s vertices together, and the opposite side the other way with Mirror', () => {
    const right = movedSide(box(), FLAT, 1, [5, 0], false).points;
    expect(right).toEqual([
      [40, 40],
      [65, 40],
      [65, 60],
      [40, 60],
    ]);
    const both = movedSide(box(), FLAT, 1, [5, 0], true).points;
    expect(both?.[0]).toEqual([35, 40]);
    expect(freeSides(FLAT).map((side) => side.point)).toEqual([
      [40, 50],
      [60, 50],
      [50, 40],
      [50, 60],
    ]);
  });

  it("tells a 3D box's faces toward the camera by how they wind, as its solid would", () => {
    const solid = box({ solid: { thickness: 20, tip: 20, swing: 25 } });
    const corners = solidCorners(solid, solid.solid!);
    const facing = solidFaces(solid, solid.solid!).map((face) => face.facing);
    expect(freeSides(corners).map((side) => side.seen)).toEqual(facing);
    expect(freeEdges(corners).filter((edge) => !edge.seen)).toHaveLength(3);
  });
});
