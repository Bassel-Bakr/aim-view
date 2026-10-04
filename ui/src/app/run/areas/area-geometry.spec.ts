import { AreaBox } from '../../api';
import { recordingContext } from '../recording-context';
import {
  AreaScene,
  AreaStyle,
  dragArea,
  drawAreas,
  drawnArea,
  holdAt,
  nextUnder,
  sameAreas,
  ScreenSize,
} from './area-geometry';

const SIZE: ScreenSize = { width: 1000, height: 500 };
const AREA_A: AreaBox = [0.1, 0.1, 0.5, 0.5, 'webcam'];
const AREA_B: AreaBox = [0.3, 0.3, 0.7, 0.7, 'other'];

describe('area geometry', () => {
  it('takes hold of the selected area first, else the topmost, by its inside or its edges', () => {
    expect(holdAt([AREA_A, AREA_B], -1, [0.4, 0.4], SIZE)).toEqual({ index: 1, edges: null });
    expect(holdAt([AREA_A, AREA_B], 0, [0.4, 0.4], SIZE)).toEqual({ index: 0, edges: null });
    // 3 px from the left edge (6 px is the reach), at the top left corner
    expect(holdAt([AREA_A], -1, [0.103, 0.101], SIZE)?.edges).toEqual({
      left: true,
      right: false,
      top: true,
      bottom: false,
    });
    expect(holdAt([AREA_A], -1, [0.9, 0.9], SIZE)).toBeNull();
  });

  it('moves an area whole, kept on screen, and resizes it by its edge, at least 8 px across', () => {
    const moved = dragArea([0.1, 0.1, 0.5, 0.5], null, [0.7, -0.05], SIZE);
    expect(moved.map((value) => +value.toFixed(6))).toEqual([0.6, 0.05, 1, 0.45]);
    const edges = { left: false, right: true, top: false, bottom: false };
    const narrow = dragArea([0.1, 0.1, 0.5, 0.5], edges, [-0.5, 0], SIZE);
    expect(narrow[2]).toBeCloseTo(0.1 + 8 / 1000);
  });

  it('selects the next area under the point, round the stack, where areas overlap', () => {
    expect(nextUnder([AREA_A, AREA_B], 1, [0.4, 0.4])).toBe(0);
    expect(nextUnder([AREA_A, AREA_B], 0, [0.4, 0.4])).toBe(1);
    expect(nextUnder([AREA_A, AREA_B], 0, [0.2, 0.2])).toBe(-1);
  });

  it('draws an area from a drag either way, and takes a short one for a click', () => {
    expect(drawnArea([0.5, 0.6], [0.2, 0.1], SIZE)).toEqual([0.2, 0.1, 0.5, 0.6]);
    expect(drawnArea([0.5, 0.5], [0.503, 0.505], SIZE)).toBeNull();
    expect(drawnArea([0.9, 0.9], [1.2, 1.1], SIZE)).toEqual([0.9, 0.9, 1, 1]);
  });

  it('compares areas by where they are, not what they are', () => {
    expect(sameAreas([AREA_A], [[0.1, 0.1, 0.5, 0.5]])).toBe(true);
    expect(sameAreas([AREA_A], [AREA_B])).toBe(false);
    expect(sameAreas([AREA_A], [])).toBe(false);
  });
});

describe('drawAreas', () => {
  const style: AreaStyle = {
    fill: 'rgba(0, 0, 0, 0.3)',
    fillSelected: 'rgba(0, 0, 255, 0.3)',
    line: 'white',
    lineSelected: 'blue',
    lineWidth: 1,
    lineWidthSelected: 2,
    labelFont: '11px sans-serif',
    labelBg: 'black',
    labelText: 'white',
    labelTextSelected: 'blue',
  };

  // pinned by the calls it makes on a recording context (a digest of them): the same calls draw the same pixels
  it('draws each area with its label, the selected one see-through, the moving one and the new one unlabelled', () => {
    const drawings = [
      { boxes: [AREA_A, AREA_B], selected: -1, drawing: null, moving: -1 },
      { boxes: [AREA_A, AREA_B], selected: 1, drawing: [0.6, 0.1, 0.9, 0.2], moving: 0 },
    ].map((scene) => {
      const recording = recordingContext(SIZE.width, SIZE.height);
      drawAreas(
        recording.context,
        { ...scene, kindName: (id) => `kind ${id}` } as AreaScene,
        SIZE,
        style,
      );
      return `${recording.calls.length} ${recording.digest()}`;
    });
    expect(drawings).toEqual(['34 7b2e2572', '24 b6a0b342']);
  });
});
