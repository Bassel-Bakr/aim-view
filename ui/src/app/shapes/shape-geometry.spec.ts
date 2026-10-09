import { Shape } from '../api';
import {
  boxAround,
  corners,
  faced,
  faceHandle,
  flatSides,
  pushedFlatSide,
  evened,
  moved,
  onShape,
  resized,
  scaled,
  tracePath,
  turned,
  turnedBy,
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
    solid: null,
    points: null,
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

  it('keeps both sides equal with Shift, or evens them to their mean', () => {
    const even = resized(shape({}), 2, [70, 60], true);
    expect(even.box).toEqual([55, 60, 30, 30]);
    expect(evened(shape({})).box).toEqual([50, 50, 15, 15]);
  });

  it('turns toward the finger, moves, and gives a box or a pill its third face', () => {
    expect(turned(shape({}), [60, 50]).angle).toBe(90);
    expect(turned(shape({}), [50, 40]).angle).toBe(0);
    expect(moved(shape({}), [3, -2]).box).toEqual([53, 48, 20, 10]);
    const cube = faced(shape({ kind: 'box' }), [56, 44]);
    expect(cube.face).toEqual([6, -6]);
    expect(faceHandle(cube)).toEqual([56, 44]);
    expect(faceHandle(shape({}))).toBeNull();
    expect(onShape(cube, [65, 39], 0)).toBe(true);
    const deepPill = faced(shape({}), [70, 50]);
    expect(onShape(deepPill, [79, 50], 0) && !onShape(shape({}), [79, 50], 0)).toBe(true);
    expect(boxAround([deepPill])).toEqual([60, 50, 40, 10]);
  });

  it("turns a 3D shape's far end with it, by a step or toward the finger", () => {
    const deep = shape({ face: [10, 0] });
    const quarter = turnedBy(deep, 90);
    expect(quarter.angle).toBe(90);
    expect(quarter.face?.map((value) => Math.round(value * 1e6) / 1e6)).toEqual([0, 10]);
    expect(turnedBy(shape({ angle: 10 }), -15).angle).toBe(355);
    const followed = turned(deep, [60, 50]);
    expect(followed.angle).toBe(90);
    expect(followed.face?.map((value) => Math.round(value * 1e6) / 1e6)).toEqual([0, 10]);
  });

  it('scales a shape about a point, its face too, never below the smallest side', () => {
    const cube = shape({ kind: 'box', face: [4, -2] });
    expect(scaled(cube, 2, [40, 50])).toMatchObject({ box: [60, 50, 40, 20], face: [8, -4] });
    expect(scaled(shape({}), 0.01, [50, 50]).box).toEqual([50, 50, 4, 2]);
  });

  it('gives the box round joined shapes', () => {
    const head = shape({ box: [50, 40, 10, 10] });
    const body = shape({ box: [50, 55, 10, 20] });
    expect(boxAround([head, body])).toEqual([50, 50, 10, 30]);
  });
});

describe('an oval', () => {
  it('resizes its width and height on their own, by a corner or a side, and turns', () => {
    const oval = shape({ kind: 'ellipse' });
    expect(resized(oval, 2, [70, 70]).box).toEqual([55, 57.5, 30, 25]);
    expect(pushedFlatSide(oval, 3, [50, 65], false).box).toEqual([50, 55, 20, 20]);
    expect(turned(oval, [60, 50]).angle).toBe(90);
    expect(onShape(oval, [59, 54], 0) && !onShape(oval, [61, 50], 0)).toBe(true);
  });

  it('traces its turned ellipse on the canvas', () => {
    const calls: unknown[][] = [];
    const record =
      (name: string) =>
      (...args: unknown[]) =>
        calls.push([name, ...args]);
    const context = {
      beginPath: record('beginPath'),
      moveTo: record('moveTo'),
      ellipse: record('ellipse'),
    } as unknown as CanvasRenderingContext2D;
    tracePath(context, shape({ kind: 'ellipse', angle: 90 }));
    const [, , ellipse] = calls;
    expect(ellipse.slice(0, 5)).toEqual(['ellipse', 50, 50, 10, 5]);
    expect(ellipse[5]).toBeCloseTo(Math.PI / 2);
    expect(ellipse.slice(6)).toEqual([0, 2 * Math.PI]);
  });
});

describe("a flat box's sides", () => {
  it('moves one side of a flat box, the opposite side staying, or both with Mirror', () => {
    const flat = shape({ kind: 'box' });
    expect(flatSides(flat).map((handle) => handle.point)).toEqual([
      [40, 50],
      [60, 50],
      [50, 45],
      [50, 55],
    ]);
    expect(pushedFlatSide(flat, 1, [70, 50], false).box).toEqual([55, 50, 30, 10]);
    expect(pushedFlatSide(flat, 1, [70, 50], true).box).toEqual([50, 50, 40, 10]);
    expect(
      pushedFlatSide(shape({ kind: 'box', angle: 90 }), 2, [60, 50], false).box[3],
    ).toBeCloseTo(15);
    expect(flatSides(shape({}))).toHaveLength(4);
    expect(flatSides(shape({ face: [5, 0] }))).toEqual([]);
  });
});
