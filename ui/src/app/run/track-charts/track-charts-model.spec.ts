import { AroundPoint, Motion, TrackFrame, TrackReport, Tracks, TurnBack } from '../../api';
import { fingerprint } from '../recording-context';
import { Timeline, TrackState } from '../track';
import { aroundMap, distanceSpread, onTargetWindows, turnsBack } from './track-charts-model';

const FPS = 10;

/** A timeline of the given states, one per frame. */
const timeline = (states: TrackState[]): Timeline => ({
  start: 0,
  frameCount: states.length,
  fps: FPS,
  state: Int8Array.from(states),
  outsideDeg: new Float32Array(states.length),
  capDeg: 1,
  deaths: [],
});

const report = { summary: { on_target: 0.5 } } as unknown as TrackReport;

describe('onTargetWindows', () => {
  it('counts each 10 s of the time tracking, leaving out switching and no bot', () => {
    // 10 s: half on, half off; 10 s: on, but most of it switching; 4 s more (a last, short stretch): all on
    const states = [
      ...new Array<TrackState>(50).fill(TrackState.On),
      ...new Array<TrackState>(50).fill(TrackState.Off),
      ...new Array<TrackState>(30).fill(TrackState.On),
      ...new Array<TrackState>(70).fill(TrackState.Switching),
      ...new Array<TrackState>(40).fill(TrackState.On),
    ];
    const model = onTargetWindows(report, timeline(states));
    expect(model.bars.map((b) => b.title)).toEqual([
      '0 to 10 s: 50% on the bot',
      '10 to 20 s: 100% on the bot',
      '20 to 24 s: 100% on the bot',
    ]);
    expect(model.bars[0].seconds).toBe(0);
    expect(model.overall?.label).toBe('whole run 50%');
  });

  it('leaves out a stretch with under 2 s of tracking and a last one under 3 s', () => {
    const states = [
      ...new Array<TrackState>(85).fill(TrackState.NoBot),
      ...new Array<TrackState>(15).fill(TrackState.On),
      ...new Array<TrackState>(20).fill(TrackState.On),
    ];
    expect(onTargetWindows(report, timeline(states)).bars).toHaveLength(0);
  });
});

describe('distanceSpread', () => {
  it("spreads the distances from the bot's center line, near it only", () => {
    // a sphere 0.6 degrees across: 0.1 degrees off its center in 3 frames, 0.5 in one, 5 (too far) in one
    const frame = (x: number): TrackFrame => ({
      i: 0,
      shift: [0, 0],
      // eslint-disable-next-line id-length -- the core names TrackFrame's fields (generated/track-frame.ts)
      t: [[1, x, 0]],
      a: [1],
      wh: [[0.6, 0.6]],
    });
    const frames = [0.1, 0.1, 0.1, 0.5, 5].map(frame);
    const tracks = { fps: FPS, frames } as Tracks;
    const model = distanceSpread(tracks, timeline(new Array<TrackState>(5).fill(TrackState.Off)));
    const total = model.bars.reduce((sum, b) => sum + b.height, 0);
    expect(total).toBeGreaterThan(0);
    expect(model.median?.label).toBe('median 0.10°');
    expect(model.edge?.label).toBe('its edge 0.30°');
  });
});

describe('aroundMap', () => {
  it('maps where the crosshair sat around the moving bot: ahead to the right, behind to the left', () => {
    const ahead: AroundPoint[] = new Array<AroundPoint>(30).fill([0.4, 0, 0.3]);
    const behind: AroundPoint[] = new Array<AroundPoint>(10).fill([-0.4, 0, 0.3]);
    const motion = { camera: 1, around: [...ahead, ...behind] } as Motion;
    const model = aroundMap({ summary: { motion } } as unknown as TrackReport);
    expect(model.reason).toBeNull();
    const [right, left] = [...model.cells].sort((a, b) => b.x - a.x);
    expect(right.x).toBeGreaterThan(model.center.x);
    expect(left.x + left.size).toBeLessThan(model.center.x);
    expect(right.shade).toBe(1);
    expect(left.shade).toBeCloseTo(Math.sqrt(1 / 3), 6);
    expect(right.title).toContain('ahead');
    expect(model.usual?.x).toBeGreaterThan(model.center.x);
  });

  it('says why there is no map', () => {
    const map = (motion: Motion | null) =>
      aroundMap({ summary: { motion } } as unknown as TrackReport).reason;
    expect(map({ camera: 1, reason: 'too little tracking' } as Motion)).toBe(
      'Not measured: too little tracking.',
    );
    expect(map({ camera: 1 } as Motion)).toContain('review it again');
  });
});

describe('turnsBack', () => {
  it('times the way back onto the bot after each turn, with the median and the quick share', () => {
    // a run from frame 20, 30 s: back after 0.1 s, stayed on, back after 0.3 s, not back before the next turn
    const turns_back = [
      { frame: 40, back: 0.1 },
      { frame: 70, back: 0 },
      { frame: 120, back: 0.3 },
      { frame: 200, back: null },
    ];
    const motion = { camera: 1, turns_back } as Motion;
    const run = { ...timeline(new Array<TrackState>(300).fill(TrackState.On)), start: 20 };
    const model = turnsBack({ summary: { motion } } as unknown as TrackReport, run);
    expect(model.reason).toBeNull();
    expect(model.dots.map((dot) => dot.seconds)).toEqual([4, 7, 12, 20]);
    expect(model.dots[0].x).toBeLessThan(model.dots[1].x);
    expect(model.dots[1].y).toBe(model.box.height - model.box.bottom);
    expect(model.dots[0].y).toBeGreaterThan(model.dots[2].y);
    expect(model.dots[3].lost).toBe(true);
    expect(model.dots[3].y).toBe(model.box.top);
    expect(model.dots[0].title).toBe('Turn at 2 s: back on the bot after 100 ms');
    expect(model.median?.label).toBe('median 100 ms');
    expect(model.note).toBe(
      'Back on the bot a median 100 ms after a turn. Back within 200 ms after 50% of the 4 turns. ' +
        '1 not back before the next turn.',
    );
  });

  it('says why there is no turns chart', () => {
    const reason = (motion: Motion) =>
      turnsBack({ summary: { motion } } as unknown as TrackReport, timeline([])).reason;
    expect(reason({ camera: 1, reason: 'too little tracking' } as Motion)).toBe(
      'Not measured: too little tracking.',
    );
    expect(reason({ camera: 1 } as Motion)).toContain('review it again');
    expect(reason({ camera: 1, turns_back: [] as TurnBack[] } as Motion)).toContain(
      'did not change direction',
    );
  });
});

describe('track charts', () => {
  it('keeps every number of each chart (a digest of each model as JSON)', () => {
    const states = Array.from(
      { length: 650 },
      (_unused, i): TrackState =>
        [TrackState.On, TrackState.On, TrackState.Off, TrackState.Switching, TrackState.NoBot][
          Math.floor(i / 9) % 5
        ],
    );
    const run = { ...timeline(states), start: 15 };
    const frames = Array.from({ length: 700 }, (_unused, i): TrackFrame => ({
      i,
      shift: [0, 0],
      // eslint-disable-next-line id-length -- the core names TrackFrame's fields (generated/track-frame.ts)
      t: [[1, ((i * 13) % 17) / 10 - 0.8, ((i * 7) % 11) / 20 - 0.25]],
      a: [1],
      wh: [[0.5 + (i % 4) / 10, 0.6 + (i % 3) / 5]],
    }));
    const around = Array.from({ length: 240 }, (_unused, i): AroundPoint => [
      ((i * 29) % 41) / 25 - 0.8,
      ((i * 17) % 23) / 30 - 0.35,
      0.3 + (i % 5) / 50,
    ]);
    const turns_back = Array.from({ length: 30 }, (_unused, i) => ({
      frame: 20 + 21 * i,
      back: i % 7 === 3 ? null : ((i * 11) % 9) / 20,
    }));
    const motion = { camera: 1, around, turns_back } as Motion;
    const withMotion = { summary: { on_target: 0.62, motion } } as unknown as TrackReport;
    const models = [
      onTargetWindows(withMotion, run),
      distanceSpread({ fps: FPS, frames } as Tracks, run),
      aroundMap(withMotion),
      turnsBack(withMotion, run),
    ];
    expect(models.map((model) => fingerprint(JSON.stringify(model)))).toEqual([
      'bcba74e0',
      '2931d144',
      '40f19a87',
      'ca2f24c5',
    ]);
  });
});
