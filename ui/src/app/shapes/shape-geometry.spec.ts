import { Shape } from '../api';
import {
  boxAround,
  corners,
  faced,
  faceHandle,
  moved,
  onShape,
  resized,
  turned,
  turnHandle,
} from './shape-geometry';

/** A shape for the tests: a pill at (50, 50), 20 wide and 10 tall, unless overrides say otherwise. */
function shape(overrides: Partial<Shape>): Shape {
  return {
    id: 's',
    kind: 'pill',
    box: [50, 50, 20, 10],
    angle: 0,
    face: null,
    depth: 0,
    role: null,
    model: null,
    ...overrides,
  };
}

const near = (a: number, b: number) => expect(a).toBeCloseTo(b, 6);

describe('shape geometry', () => {
  it('turns a shape about its middle, clockwise on screen', () => {
    const upright = shape({ angle: 90 });
    const [topLeft] = corners(upright);
    near(topLeft[0], 55);
    near(topLeft[1], 40);
    expect(onShape(upright, [50, 59], 0)).toBe(true);
    expect(onShape(upright, [59, 50], 0)).toBe(false);
    const [hx, hy] = turnHandle(upright, 10);
    near(hx, 65);
    near(hy, 50);
  });

  it('resizes from a corner and keeps the opposite one where it was', () => {
    const before = shape({ angle: 30 });
    const after = resized(before, 2, [70, 70]);
    near(corners(after)[0][0], corners(before)[0][0]);
    near(corners(after)[0][1], corners(before)[0][1]);
    near(corners(after)[2][0], 70);
    near(corners(after)[2][1], 70);
  });

  it('turns toward the finger, moves, and gives a box its third face', () => {
    expect(turned(shape({}), [60, 50]).angle).toBe(90);
    expect(turned(shape({}), [50, 40]).angle).toBe(0);
    expect(moved(shape({}), [3, -2]).box).toEqual([53, 48, 20, 10]);
    const cube = faced(shape({ kind: 'box' }), [56, 44]);
    expect(cube.face).toEqual([6, -6]);
    expect(faceHandle(cube, 10)).toEqual([56, 44]);
    expect(faceHandle(shape({}), 10)).toBeNull();
    expect(onShape(cube, [65, 39], 0)).toBe(true);
  });

  it('gives the box round joined shapes', () => {
    const head = shape({ box: [50, 40, 10, 10] });
    const body = shape({ box: [50, 55, 10, 20] });
    expect(boxAround([head, body])).toEqual([50, 50, 10, 30]);
  });
});
