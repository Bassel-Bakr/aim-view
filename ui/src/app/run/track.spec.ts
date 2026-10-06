import {
  Flick,
  Geometry,
  Hitbox,
  TargetSize,
  TrackFrame,
  TrackPoint,
  TrackReport,
  Tracks,
} from '../api';
import {
  boxes,
  clock,
  describe as describeMoment,
  flickAt,
  nearest,
  onShape,
  targetShape,
  timeline,
  toPx,
  TrackState,
} from './track';

// eslint-disable-next-line id-length -- the core names Geometry's fields (generated/geometry.ts)
const GEOMETRY: Geometry = { W: 1280, H: 720, CX: 640, CY: 360, K: 509 };

/** A frame of tracks: each target's id and place in degrees, and its box's size where the model gave one. */
function trackFrame(index: number, targets: TrackPoint[], sizes?: TargetSize[]): TrackFrame {
  // eslint-disable-next-line id-length -- the core names TrackFrame's fields (generated/track-frame.ts)
  return { i: index, shift: [0, 0], t: targets, a: targets.map(() => 1), wh: sizes };
}

describe('toPx', () => {
  it('puts the crosshair at the crosshair pixel, scaled', () => {
    expect(toPx(GEOMETRY, 0, 0, 0.5)).toEqual([320, 180]);
  });

  it('puts a target to the right and above at a larger x and a smaller y', () => {
    const [x, y] = toPx(GEOMETRY, 10, 5, 1);
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
    const targetBoxes = boxes(
      trackFrame(
        0,
        [
          [1, 0.1, 0],
          [2, 3, 0],
        ],
        [
          [1, 1],
          [1, 1],
        ],
      ),
    );
    expect(targetBoxes[0].inside).toBe(true);
    expect(targetBoxes[1].inside).toBe(false);
    expect(targetBoxes[1].outsideDeg).toBeCloseTo(2.5);
    expect(nearest(targetBoxes)).toBe(targetBoxes[0]);
  });

  it('measures a capsule from its long axis, not its center', () => {
    // 4 deg tall and 0.5 wide, 2 above the crosshair: its axis runs from 0.25 to 3.75 above it
    const [capsule] = boxes(trackFrame(0, [[1, 0, 2]], [[0.5, 4]]));
    expect(capsule.centerLineDeg).toBeCloseTo(0.25);
  });
});

describe('targetShape and onShape', () => {
  const sphere: Hitbox = { kind: 'spheroid', widthToHeight: 1 };
  const capsule: Hitbox = { kind: 'cylindrical', widthToHeight: 0.25 };

  it('keeps a plain box without a hitbox, as before', () => {
    const shape = targetShape(1, 2, null);
    expect(shape).toEqual({ kind: 'box', halfWidthDeg: 0.5, halfHeightDeg: 1 });
    expect(onShape(0.5, 1, shape)).toBe(true);
  });

  it("leaves out a sphere's corners", () => {
    // a sphere 2 deg across: its box's corner is on the box but off the ball
    const shape = targetShape(2, 2, sphere);
    expect(shape.kind).toBe('ellipse');
    expect(onShape(0.9, 0.9, shape)).toBe(false);
    expect(onShape(0.9, 0, shape)).toBe(true);
  });

  it("sizes a capsule from its height and the hitbox's ratio, its round ends left out at the corners", () => {
    // the box 1 wide (blur) and 4 tall: the capsule is 4 tall and a quarter of that wide
    const shape = targetShape(1, 4, capsule);
    expect(shape).toEqual({ kind: 'capsule', halfWidthDeg: 0.5, halfHeightDeg: 2 });
    expect(onShape(0.45, 0, shape)).toBe(true);
    expect(onShape(0.45, 1.98, shape)).toBe(false);
  });

  it('gives the boxes of a frame their shape and the core on-target test', () => {
    const [ball] = boxes(trackFrame(0, [[1, 0.9, 0.9]], [[2, 2]]), sphere);
    expect(ball.shape.kind).toBe('ellipse');
    expect(ball.inside).toBe(false);
    expect(boxes(trackFrame(0, [[1, 0.9, 0.9]], [[2, 2]]))[0].inside).toBe(true);
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
      trackFrame(0, []),
      trackFrame(1, [[1, 0, 0]], [[1, 1]]),
      trackFrame(2, [[1, 2, 0]], [[1, 1]]),
      trackFrame(3, [[2, 0, 0]], [[1, 1]]),
      trackFrame(4, []),
      trackFrame(5, [[1, 0, 0]], [[1, 1]]),
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
    expect(tl.outsideDeg[0]).toBe(0);
    expect(tl.outsideDeg[1]).toBeCloseTo(1.5);
    expect(Number.isNaN(tl.outsideDeg[2])).toBe(true);
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
