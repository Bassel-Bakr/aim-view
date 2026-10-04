import { Geometry, Report, TrackFrame, Tracks } from '../../api';
import { recordingContext } from '../recording-context';
import { drawFaint, FaintLayer, FaintStyle, pointedTrack } from './faint-overlay';

// The cut-off's drawing is pinned by the calls it makes on a recording context (a digest of them): the same calls draw
// the same pixels.

// eslint-disable-next-line id-length -- the core names Geometry's fields (generated/geometry.ts)
const GEOMETRY: Geometry = { W: 1280, H: 720, CX: 640, CY: 360, K: 509 };
const SCALE = 0.5;
const REPORT = { mode: 'track', fps: 60, geometry: GEOMETRY } as unknown as Report;
const STYLE: FaintStyle = {
  font: '12px sans-serif',
  ring: 'gray',
  picked: 'gold',
  radius: 14,
  labelBg: 'black',
  hoverBg: 'navy',
  text: 'white',
  textOut: 'silver',
};

/** A frame with four tracks, each with its id, place in degrees and score. */
function frame(index: number): TrackFrame {
  return {
    i: index,
    shift: [0, 0],
    // eslint-disable-next-line id-length -- the core names TrackFrame's fields (generated/track-frame.ts)
    t: [
      [1, -10, 4],
      [2, 3, -2],
      [3, 20, 8],
      [4, 0.5, 0.2],
    ],
    a: [1, 1, 1, 1],
    // eslint-disable-next-line id-length -- the core names TrackFrame's fields (generated/track-frame.ts)
    s: [0.91, 0.12, 0.33, 0.7],
  };
}

const TRACKS: Tracks = { fps: 60, frames: [frame(0), frame(1)] };
const SCORES = new Map([
  [1, 0.88],
  [2, 0.1],
  [4, 0.65],
]);

function drawing(layer: Partial<FaintLayer>): string {
  const recording = recordingContext(640, 360);
  const full: FaintLayer = {
    scores: SCORES,
    dropped: new Set([2, 3]),
    highlight: null,
    showScores: false,
    hover: null,
    ...layer,
  };
  drawFaint(recording.context, REPORT, TRACKS, 1, SCALE, STYLE, full);
  return `${recording.calls.length} ${recording.digest()}`;
}

describe('drawFaint', () => {
  it('rings the tracks left out, the picked one apart, with the scores and the hovered point', () => {
    const hover = pointedTrack(REPORT, TRACKS, 1, SCALE, 330, 175, SCORES);
    const farRight = hover && { ...hover, x: 630 };
    expect([
      drawing({}),
      drawing({ highlight: 4, showScores: true }),
      drawing({ hover }),
      drawing({ hover: farRight }),
    ]).toEqual(['16 ffbcd3f2', '43 9760cc28', '21 ea5c0c30', '21 8faeb5dc']);
  });
});

describe('pointedTrack', () => {
  it('finds the track under the mouse, with its frame score and its own', () => {
    expect([
      pointedTrack(REPORT, TRACKS, 1, SCALE, 330, 175, SCORES),
      pointedTrack(REPORT, TRACKS, 1, SCALE, 10, 10, SCORES),
    ]).toEqual([
      {
        id: 4,
        x: 322.2209878527481,
        y: 179.11158997460223,
        frame: 1,
        text: 'track 4 · this frame 0.70 · track 0.65',
      },
      null,
    ]);
  });
});
