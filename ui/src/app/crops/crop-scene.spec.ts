import { CropAnswer, CropEntry } from '../api';
import {
  answerOf,
  inFront,
  joined,
  removed,
  sceneOfAnswer,
  sceneOfFix,
  split,
  toggledOccluders,
  uncrossed,
  withChanged,
  withDrawn,
  withRole,
} from './crop-scene';

const CROP: CropEntry = {
  id: 'c1',
  set: 'bars',
  file: 'a.mp4',
  folder: 'Air',
  kind: 'static',
  why: [],
  rule: null,
  boxes: [
    [50, 50, 20, 20],
    [150, 150, 10, 30],
  ],
  scores: [0.9, 0.4],
};

/** An answer of the claude.ai page: no scene, the old fields only. */
function oldAnswer(overrides: Partial<CropAnswer>): CropAnswer {
  return {
    verdict: 'wrong',
    set: 'bars',
    file: 'a.mp4',
    at: 1,
    remove: [],
    edit: {},
    add: [],
    ...overrides,
  };
}

describe('crop scenes', () => {
  it("makes the model's boxes pills, less those crossed out, and reads an old answer's points", () => {
    const scene = sceneOfAnswer(
      CROP,
      oldAnswer({ remove: [1], edit: { 0: [52, 50, 20, 20] }, add: [[10, 20]] }),
    );
    expect(scene.shapes.map((shape) => [shape.id, shape.kind, shape.box, shape.model])).toEqual([
      ['m0', 'pill', [52, 50, 20, 20], 0],
      ['p0', 'pill', [10, 20, 30, 30], null],
    ]);
    expect(scene.crossed).toEqual([1]);
  });

  it('writes the old fields from the scene, so the suggestions keep learning', () => {
    const scene = withDrawn(sceneOfFix(CROP, null), 'box', [150, 150, 20, 40]);
    const moved = withChanged(scene, { ...scene.shapes[0], box: [60, 50, 20, 20] });
    const answer = answerOf(CROP, moved, 'wrong', false);
    expect(answer.remove).toEqual([1]);
    expect(answer.edit).toEqual({ 0: [60, 50, 20, 20] });
    expect(answer.add).toEqual([[150, 150, 20, 40]]);
    expect(answer.scene?.shapes.map((shape) => shape.id)).toEqual(['m0', 's2']);
    expect(answer.suggested).toBeUndefined();
  });

  it('keeps occluders out of the old fields, and every number to a tenth of a pixel', () => {
    let scene = withDrawn(sceneOfFix(CROP, null), 'box', [20, 20, 10, 10]);
    scene = withChanged(scene, { ...scene.shapes[2], face: [3.04, -2.96] });
    scene = withDrawn(toggledOccluders(scene, ['s2']), 'pill', [100.04, 60, 10.01, 10]);
    const answer = answerOf(CROP, scene, 'wrong', false);
    expect(answer.add).toEqual([[100, 60, 10, 10]]);
    expect(answer.scene?.shapes.find((shape) => shape.id === 's2')?.face).toEqual([3, -3]);
    expect(answer.scene?.occluders).toEqual(['s2']);
  });

  it('joins shapes into one target, splits them, and gives them roles', () => {
    let scene = withDrawn(sceneOfFix(CROP, null), 'pill', [50, 80, 30, 30]);
    scene = withRole(joined(scene, ['m0', 's2']), ['m0'], 'head');
    expect(scene.targets).toEqual([['m0', 's2']]);
    expect(scene.shapes.find((shape) => shape.id === 'm0')?.role).toBe('head');
    expect(split(scene, ['s2']).targets).toEqual([]);
  });

  it('orders shapes front to back, and makes occluders that are no target', () => {
    let scene = joined(sceneOfFix(CROP, null), ['m0', 'm1']);
    scene = inFront(scene, ['m1'], false);
    expect(scene.shapes.map((shape) => shape.depth)).toEqual([0, -1]);
    scene = toggledOccluders(scene, ['m1']);
    expect(scene.occluders).toEqual(['m1']);
    expect(scene.targets).toEqual([]);
    expect(scene.shapes.find((shape) => shape.id === 'm1')?.depth).toBe(1);
    expect(toggledOccluders(scene, ['m1']).occluders).toEqual([]);
  });

  it('crosses a model shape out and brings it back; a drawn one is deleted', () => {
    let scene = withDrawn(sceneOfFix(CROP, null), 'pill', [10, 10, 5, 5]);
    scene = removed(scene, ['m0', 's2']);
    expect(scene.shapes.map((shape) => shape.id)).toEqual(['m1']);
    expect(scene.crossed).toEqual([0]);
    scene = uncrossed(scene, CROP, 0);
    expect(scene.crossed).toEqual([]);
    expect(scene.shapes.map((shape) => shape.id)).toEqual(['m1', 'm0']);
  });
});
