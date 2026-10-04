import { ClickReport, Flick, Geometry, TrackFrame, TrackReport, Tracks } from '../../api';
import { analysePaths } from '../fastest-path/path-analysis';
import { recordingContext } from '../recording-context';
import { drawClick, drawPaths, drawTrack, OverlayStyle } from './overlay';

// The overlay's drawings are pinned by the calls they make on a recording context (a digest of them): the same calls
// draw the same pixels.

/** No calls at all. */
const NOTHING = '0 741638a5';
// eslint-disable-next-line id-length -- the core names Geometry's fields (generated/geometry.ts)
const GEOMETRY: Geometry = { W: 1280, H: 720, CX: 640, CY: 360, K: 509 };
const SCALE = 0.75;
const STYLE: OverlayStyle = {
  font: '12px sans-serif',
  crosshair: 'white',
  otherTarget: 'gray',
  ring: 'yellow',
  onTarget: 'green',
  offTarget: 'orange',
  labelBg: 'black',
  labelText: 'white',
  line: 1,
  lineStrong: 2,
  fastest: 'lime',
  mine: 'coral',
  orderFont: '10px sans-serif',
  pathWidth: 3,
  legendBg: 'navy',
};

function frame(index: number, targets: TrackFrame['t'], sizes?: TrackFrame['wh']): TrackFrame {
  // eslint-disable-next-line id-length -- the core names TrackFrame's fields (generated/track-frame.ts)
  return { i: index, shift: [0, 0], t: targets, a: targets.map(() => 1), wh: sizes };
}

// Three kills; at frame 33 two targets are on screen, the far one killed first.
const FLICKS = [
  { kill_number: 1, start_frame: 30, kill_frame: 40, react: 0, D0: 9, total: 0.6 },
  { kill_number: 2, start_frame: 50, kill_frame: 60, react: 0, D0: 2, total: 0.3 },
  { kill_number: 3, start_frame: 70, kill_frame: 80, react: 0, D0: 4, total: 0.4 },
] as Flick[];
const CLICK_FRAMES: TrackFrame[] = Array.from({ length: 81 }, (_unused, i) => frame(i, []));
CLICK_FRAMES[33] = frame(33, [
  [1, 9, 1],
  [2, 2, -1],
  [3, -4, 2],
]);
CLICK_FRAMES[40] = frame(40, [[1, 0, 0]]);
CLICK_FRAMES[60] = frame(60, [[2, 0, 0]]);
CLICK_FRAMES[80] = frame(80, [[3, 0, 0]]);
const CLICK_TRACKS: Tracks = { fps: 100, frames: CLICK_FRAMES };
const CLICK_REPORT = {
  mode: 'click',
  fps: 100,
  geometry: GEOMETRY,
  flicks: FLICKS,
  paths: {
    '1': [
      [35, 4, 1],
      [40, 0, 0],
    ],
    '2': [[60, 0, 0]],
    '3': [[80, 0, 0]],
  },
  appeared: { '1': 0, '2': 0, '3': 0 },
  summary: { radius: 0.5, median_interval: 0.4, kills: 3, shots: 3, info: { source: 'stats' } },
} as unknown as ClickReport;

const TRACK_REPORT = {
  mode: 'track',
  fps: 10,
  geometry: GEOMETRY,
  summary: { start: 1, end: 9, switches: [[6, 8]] },
} as unknown as TrackReport;
const TRACK_TRACKS: Tracks = {
  fps: 10,
  frames: [
    frame(0, [[1, 0, 0]], [[1, 1]]),
    frame(1, [[1, 0.1, 0]], [[1, 2]]),
    frame(
      2,
      [
        [1, 2, 1],
        [2, -3, 0.5],
      ],
      [
        [1, 1],
        [0.5, 3],
      ],
    ),
    frame(3, [[1, 1.5, -2]]),
    frame(4, []),
    frame(5, [[1, 0, 0]], [[1, 1]]),
    frame(6, [[2, 4, 0]], [[1, 1]]),
    frame(7, [[2, 3, 0]], [[1, 1]]),
  ],
};

function clickDrawing(at: number): string {
  const recording = recordingContext();
  drawClick(recording.context, CLICK_REPORT, at, SCALE, STYLE);
  return `${recording.calls.length} ${recording.digest()}`;
}

function trackDrawing(at: number): string {
  const recording = recordingContext();
  drawTrack(recording.context, TRACK_REPORT, TRACK_TRACKS, at, SCALE, STYLE);
  return `${recording.calls.length} ${recording.digest()}`;
}

function pathsDrawing(at: number, fastest: boolean, mine: boolean): string {
  const recording = recordingContext();
  const analysis = analysePaths(CLICK_REPORT, CLICK_TRACKS);
  if (!analysis) throw new Error('no analysis');
  drawPaths(recording.context, CLICK_REPORT, CLICK_TRACKS, at, SCALE, STYLE, analysis, {
    fastest,
    mine,
  });
  return `${recording.calls.length} ${recording.digest()}`;
}

describe('drawClick', () => {
  it('draws the crosshair, the target ring, the line to it and its distance', () => {
    expect(clickDrawing(35)).toBe('23 6e4603c5');
  });

  it('draws only the crosshair where the flick has no point, and nothing far from a flick', () => {
    expect([clickDrawing(36), clickDrawing(200)]).toEqual(['5 681faddb', NOTHING]);
  });
});

describe('drawTrack', () => {
  it('draws on target, off target with its distance, and a frame with no box size', () => {
    expect([trackDrawing(1), trackDrawing(2), trackDrawing(3)]).toEqual([
      '10 bea2955e',
      '21 519cda0b',
      '18 9b421d19',
    ]);
  });

  it('says switching after a death, and draws nothing outside the run or without a bot', () => {
    expect([trackDrawing(6), trackDrawing(4), trackDrawing(0)]).toEqual([
      '18 34028dda',
      NOTHING,
      NOTHING,
    ]);
  });
});

describe('drawPaths', () => {
  it('draws both paths with their legend', () => {
    expect(pathsDrawing(33, true, true)).toBe('92 e40b6c59');
  });

  it('draws one path alone', () => {
    expect([pathsDrawing(33, true, false), pathsDrawing(33, false, true)]).toEqual([
      '48 3e9670d0',
      '48 9128b610',
    ]);
  });

  it('draws nothing outside the run', () => {
    expect(pathsDrawing(20, true, true)).toBe(NOTHING);
  });
});
