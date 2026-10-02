import { Flick, Geometry, TrackReport, Tracks } from '../api';
import {
  boxes,
  clock,
  describe as describeMoment,
  flickAt,
  nearest,
  timeline,
  toPx,
  TrackState,
} from './track';

const G: Geometry = { W: 1280, H: 720, CX: 640, CY: 360, K: 509 };

describe('toPx', () => {
  it('puts the crosshair at the crosshair pixel, scaled', () => {
    expect(toPx(G, 0, 0, 0.5)).toEqual([320, 180]);
  });

  it('puts a target to the right and above at a larger x and a smaller y', () => {
    const [x, y] = toPx(G, 10, 5, 1);
    expect(x).toBeGreaterThan(640);
    expect(y).toBeLessThan(360);
  });
});

describe('flickAt', () => {
  const flicks = [
    { start_frame: 10, kill_frame: 40 },
    { start_frame: 50, kill_frame: 90 },
  ] as Flick[];

  it('is the latest flick to have started', () => {
    expect(flickAt(flicks, 60)?.start_frame).toBe(50);
    expect(flickAt(flicks, 45)?.start_frame).toBe(10);
  });

  it('is none before the first flick', () => {
    expect(flickAt(flicks, 5)).toBeNull();
  });
});

describe('boxes and nearest', () => {
  it('finds the crosshair inside a target over it, and the distance to the edge of one beside it', () => {
    const bs = boxes({
      i: 0,
      t: [
        [1, 0.1, 0],
        [2, 3, 0],
      ],
      wh: [
        [1, 1],
        [1, 1],
      ],
    });
    expect(bs[0].inside).toBe(true);
    expect(bs[1].inside).toBe(false);
    expect(bs[1].out).toBeCloseTo(2.5);
    expect(nearest(bs)).toBe(bs[0]);
  });

  it('measures a capsule from its long axis, not its center', () => {
    // 4 deg tall and 0.5 wide, 2 above the crosshair: its axis runs from 0.25 to 3.75 above it
    const [capsule] = boxes({ i: 0, t: [[1, 0, 2]], wh: [[0.5, 4]] });
    expect(capsule.d).toBeCloseTo(0.25);
  });
});

describe('timeline', () => {
  const report = {
    mode: 'track',
    fps: 10,
    summary: { start: 1, end: 5, switches: [[3, 4]] },
  } as unknown as TrackReport;
  const tracks: Tracks = {
    fps: 10,
    frames: [
      { i: 0, t: [] },
      { i: 1, t: [[1, 0, 0]], wh: [[1, 1]] },
      { i: 2, t: [[1, 2, 0]], wh: [[1, 1]] },
      { i: 3, t: [[2, 0, 0]], wh: [[1, 1]] },
      { i: 4, t: [] },
      { i: 5, t: [[1, 0, 0]], wh: [[1, 1]] },
    ],
  };

  it('keeps the run only, frame by frame: on, off, switching after a death, no bot', () => {
    const tl = timeline(report, tracks);
    expect(tl.start).toBe(1);
    expect([...tl.state]).toEqual([
      TrackState.On,
      TrackState.Off,
      TrackState.Switching,
      TrackState.NoBot,
    ]);
    expect(tl.dist[0]).toBe(0);
    expect(tl.dist[1]).toBeCloseTo(1.5);
    expect(Number.isNaN(tl.dist[2])).toBe(true);
    expect(tl.deaths).toEqual([2]);
  });

  it('says what a moment was, with its time', () => {
    const tl = timeline(report, tracks);
    expect(describeMoment(tl, 1)).toBe('0:00.2 · off target · 1.50° outside its edge');
  });
});

describe('clock', () => {
  it('shows minutes, seconds and tenths', () => {
    expect(clock(75.25)).toBe('1:15.3');
    expect(clock(4)).toBe('0:04.0');
  });
});
