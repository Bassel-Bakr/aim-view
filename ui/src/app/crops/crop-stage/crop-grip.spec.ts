import { CropEntry, CropVertex, Shape } from '../../api';
import { DraftScene } from '../crop-scene';
import { CropGrip, dragged, gripAt, handlesOf } from './crop-grip';

/** A polygon placed vertex by vertex: a pointy pentagon whose frame is 80 by 80 round (100, 100). */
const PLACED: Shape = {
  id: 'p',
  kind: 'polygon',
  box: [100, 100, 80, 80],
  angle: 0,
  face: null,
  solid: null,
  points: [
    [100, 60],
    [140, 90],
    [120, 140],
    [70, 130],
    [60, 80],
  ],
  sides: 5,
  depth: 0,
  role: null,
  model: null,
};

const SCENE: DraftScene = { shapes: [PLACED], targets: [], occluders: [], crossed: [] };

/** The placed polygon's vertices after a grip is dragged from one point to another, to a millionth of a pixel. */
function draggedPoints(grip: CropGrip, start: CropVertex, point: CropVertex): number[][] {
  const after = dragged(SCENE, grip, ['p'], { start, point, even: false, mirror: false });
  return (after.shapes[0].points ?? []).map((vertex) =>
    vertex.map((value) => Math.round(value * 1e6) / 1e6 + 0),
  );
}

describe('a polygon placed vertex by vertex', () => {
  it('has its frame round its vertices, big enough on screen, its handles clear of the vertices', () => {
    const big = handlesOf(PLACED, 2);
    expect(big.corners).toEqual(PLACED.points);
    expect(big.frame).toEqual([[60, 60], [140, 60], [140, 140], null]);
    expect(big.sides.map((side) => [side.side, ...side.point])).toEqual([
      [0, 60, 100],
      [3, 100, 140],
    ]);
    const small = handlesOf(PLACED, 0.5);
    expect(small.frame).toEqual([]);
    expect(small.sides).toEqual([]);
  });

  it('takes a vertex before the frame corner beside it', () => {
    const crop = { boxes: [] } as unknown as CropEntry;
    expect(gripAt(SCENE, crop, ['p'], [60, 79], 2)).toEqual({ kind: 'corner', id: 'p', index: 4 });
    expect(gripAt(SCENE, crop, ['p'], [61, 61], 2)).toEqual({ kind: 'frame', id: 'p', index: 0 });
  });

  it('scales all its vertices evenly by a frame corner, the opposite corner staying put', () => {
    const grip: CropGrip = { kind: 'frame', id: 'p', index: 2 };
    expect(draggedPoints(grip, [140, 140], [100, 100])).toEqual([
      [80, 60],
      [100, 75],
      [90, 100],
      [65, 95],
      [60, 70],
    ]);
  });

  it('scales its vertices along one axis by a frame side', () => {
    const grip: CropGrip = { kind: 'side', id: 'p', index: 1 };
    expect(draggedPoints(grip, [140, 100], [180, 100])).toEqual([
      [120, 60],
      [180, 90],
      [150, 140],
      [75, 130],
      [60, 80],
    ]);
  });

  it('turns all its vertices about their middle by the turn handle', () => {
    const grip: CropGrip = { kind: 'turn', id: 'p', index: -1 };
    expect(draggedPoints(grip, [100, 30], [200, 100])).toEqual([
      [140, 100],
      [110, 140],
      [60, 120],
      [70, 70],
      [120, 60],
    ]);
  });
});
