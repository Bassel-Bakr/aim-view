import { AroundPoint, Motion, TrackFrame, TrackReport, Tracks, TurnBack } from '../../api';
import { Timeline, TrackState } from '../track';
import { aroundMap, distanceSpread, onTargetWindows, turnsBack } from './track-charts-model';

const FPS = 10;

/** A timeline of the given states, one per frame. */
const timeline = (states: TrackState[]): Timeline => ({
  start: 0,
  n: states.length,
  fps: FPS,
  state: Int8Array.from(states),
  dist: new Float32Array(states.length),
  cap: 1,
  deaths: [],
});

const report = { summary: { on_target: 0.5 } } as unknown as TrackReport;

describe('track charts', () => {
  it('counts each 10 s of the time tracking, leaving out switching and no bot', () => {
    // 10 s: half on, half off; 10 s: on, but most of it switching; 4 s more (a last, short stretch): all on
    const states = [
      ...new Array<TrackState>(50).fill(TrackState.On),
      ...new Array<TrackState>(50).fill(TrackState.Off),
      ...new Array<TrackState>(30).fill(TrackState.On),
      ...new Array<TrackState>(70).fill(TrackState.Switching),
      ...new Array<TrackState>(40).fill(TrackState.On),
    ];
    const m = onTargetWindows(report, timeline(states));
    expect(m.bars.map((b) => b.title)).toEqual([
      '0 to 10 s: 50% on the bot',
      '10 to 20 s: 100% on the bot',
      '20 to 24 s: 100% on the bot',
    ]);
    expect(m.bars[0].seconds).toBe(0);
    expect(m.overall?.label).toBe('whole run 50%');
  });

  it('leaves out a stretch with under 2 s of tracking and a last one under 3 s', () => {
    const states = [
      ...new Array<TrackState>(85).fill(TrackState.NoBot),
      ...new Array<TrackState>(15).fill(TrackState.On),
      ...new Array<TrackState>(20).fill(TrackState.On),
    ];
    expect(onTargetWindows(report, timeline(states)).bars).toHaveLength(0);
  });

  it("spreads the distances from the bot's center line, near it only", () => {
    // a sphere 0.6 degrees across: 0.1 degrees off its center in 3 frames, 0.5 in one, 5 (too far) in one
    const frame = (x: number): TrackFrame => ({
      i: 0,
      shift: [0, 0],
      t: [[1, x, 0]],
      a: [1],
      wh: [[0.6, 0.6]],
    });
    const frames = [0.1, 0.1, 0.1, 0.5, 5].map(frame);
    const tracks = { fps: FPS, frames } as Tracks;
    const m = distanceSpread(tracks, timeline(new Array<TrackState>(5).fill(TrackState.Off)));
    const total = m.bars.reduce((s, b) => s + b.height, 0);
    expect(total).toBeGreaterThan(0);
    expect(m.median?.label).toBe('median 0.10°');
    expect(m.edge?.label).toBe('its edge 0.30°');
  });

  it('maps where the crosshair sat around the moving bot: ahead to the right, behind to the left', () => {
    const ahead: AroundPoint[] = new Array<AroundPoint>(30).fill([0.4, 0, 0.3]);
    const behind: AroundPoint[] = new Array<AroundPoint>(10).fill([-0.4, 0, 0.3]);
    const motion = { camera: 1, around: [...ahead, ...behind] } as Motion;
    const m = aroundMap({ summary: { motion } } as unknown as TrackReport);
    expect(m.reason).toBeNull();
    const [right, left] = [...m.cells].sort((a, b) => b.x - a.x);
    expect(right.x).toBeGreaterThan(m.center.x);
    expect(left.x + left.size).toBeLessThan(m.center.x);
    expect(right.shade).toBe(1);
    expect(left.shade).toBeCloseTo(Math.sqrt(1 / 3), 6);
    expect(right.title).toContain('ahead');
    expect(m.usual?.x).toBeGreaterThan(m.center.x);
  });

  it('says why there is no map', () => {
    const map = (motion: Motion | null) =>
      aroundMap({ summary: { motion } } as unknown as TrackReport).reason;
    expect(map({ camera: 1, reason: 'too little tracking' } as Motion)).toBe(
      'Not measured: too little tracking.',
    );
    expect(map({ camera: 1 } as Motion)).toContain('review it again');
  });

  it('times the way back onto the bot after each turn, with the median and the quick share', () => {
    // a run from frame 20, 30 s: back after 0.1 s, stayed on, back after 0.3 s, not back before the next turn
    const turns_back = [
      { frame: 40, back: 0.1 },
      { frame: 70, back: 0 },
      { frame: 120, back: 0.3 },
      { frame: 200, back: null },
    ];
    const motion = { camera: 1, turns_back } as Motion;
    const tl = { ...timeline(new Array<TrackState>(300).fill(TrackState.On)), start: 20 };
    const m = turnsBack({ summary: { motion } } as unknown as TrackReport, tl);
    expect(m.reason).toBeNull();
    expect(m.dots.map((d) => d.seconds)).toEqual([4, 7, 12, 20]);
    expect(m.dots[0].x).toBeLessThan(m.dots[1].x);
    expect(m.dots[1].y).toBe(m.box.height - m.box.bottom);
    expect(m.dots[0].y).toBeGreaterThan(m.dots[2].y);
    expect(m.dots[3].lost).toBe(true);
    expect(m.dots[3].y).toBe(m.box.top);
    expect(m.dots[0].title).toBe('Turn at 2 s: back on the bot after 100 ms');
    expect(m.median?.label).toBe('median 100 ms');
    expect(m.note).toBe(
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
