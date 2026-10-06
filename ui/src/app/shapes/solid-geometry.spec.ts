import { Shape, Solid } from '../api';
import { corners, resized } from './shape-geometry';
import {
  frontCorners,
  onSolid,
  pillAxis,
  pushedSide,
  showing,
  sideHandles,
  solidEdges,
  tumbled,
  tumbleHandle,
} from './solid-geometry';

/** A box at (50, 50), 20 wide, 10 tall and 30 thick, facing the camera unless `solid` says otherwise. */
function box(solid: Partial<Solid> = {}, angle = 0): Shape {
  return {
    id: 's',
    kind: 'box',
    box: [50, 50, 20, 10],
    angle,
    face: null,
    solid: { thickness: 30, tip: 0, swing: 0, ...solid },
    depth: 0,
    role: null,
    model: null,
  };
}

const rounded = (points: number[][]) =>
  points.map((point) => point.map((value) => Math.round(value * 1e6) / 1e6));

describe('solid geometry', () => {
  it('facing the camera, a solid box is its rectangle, and resizes as one', () => {
    const facing = box();
    const flat = { ...facing, solid: null };
    expect(rounded(frontCorners(facing, facing.solid!))).toEqual(rounded(corners(flat)));
    expect(rounded([resized(facing, 2, [65, 60]).box])).toEqual(
      rounded([resized(flat, 2, [65, 60]).box]),
    );
    expect(sideHandles(facing, facing.solid!).map((handle) => handle.side)).toEqual([0, 1, 2, 3]);
  });

  it('shows its top when tipped and its side when swung, as the core draws it', () => {
    const tipped = box({ tip: 90 });
    expect(
      onSolid(tipped, tipped.solid!, [50, 64.9], 0) &&
        !onSolid(tipped, tipped.solid!, [50, 65.5], 0),
    ).toBe(true);
    const swung = box({ swing: 90 });
    expect(
      onSolid(swung, swung.solid!, [64.9, 50], 0) && !onSolid(swung, swung.solid!, [65.5, 50], 0),
    ).toBe(true);
    const ball = {
      ...box({ tip: 90 }),
      kind: 'pill' as const,
      box: [50, 50, 10, 40] as Shape['box'],
    };
    expect(rounded(pillAxis(ball, ball.solid!))).toEqual([
      [50, 50],
      [50, 50],
    ]);
  });

  it('tumbles under a drag as a rolled ball: down shows its top, right turns its right side away', () => {
    const facing = box();
    const down = tumbled(facing, facing.solid!, [0, 15]);
    expect(down.solid!.tip).toBeCloseTo(90);
    const right = tumbled(facing, facing.solid!, [7.5, 0]);
    expect(right.solid!.swing).toBeCloseTo(-45);
    expect(right.angle).toBeCloseTo(0);
    const back = tumbled(right, right.solid!, [-7.5, 0]);
    expect(back.solid!.swing).toBeCloseTo(0);
    const [x, y] = tumbleHandle(facing, facing.solid!, 4);
    expect(x).toBeCloseTo(50);
    expect(y).toBeGreaterThan(65);
  });

  it('moves any face, the opposite one staying or mirrored, and dashes the edges the camera cannot see', () => {
    const swung = box({ swing: 30 });
    const back = sideHandles(swung, swung.solid!).find((handle) => handle.side === 5)!;
    expect(back.seen).toBe(false);
    expect(rounded([back.point])).toEqual(rounded([[50 + 15 * Math.sin(Math.PI / 6), 50]]));
    const pushed = pushedSide(swung, swung.solid!, 5, [60, 50], { minSide: 2, mirror: false });
    expect(pushed.solid!.thickness).toBeCloseTo(35);
    expect(pushed.box[0]).toBeCloseTo(51.25);
    const mirrored = pushedSide(swung, swung.solid!, 5, [60, 50], { minSide: 2, mirror: true });
    expect([mirrored.solid!.thickness, mirrored.box[0]]).toEqual([40, 50]);
    const corner = box({ tip: 20, swing: 30 });
    expect(solidEdges(corner, corner.solid!).filter((edge) => !edge.seen)).toHaveLength(3);
    expect(showing(box(), box().solid!, 'top', 15).solid!.tip).toBeCloseTo(15);
  });
});
