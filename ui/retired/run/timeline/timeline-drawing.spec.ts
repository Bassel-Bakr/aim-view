import { recordingContext } from '../recording-context';
import { Timeline, TrackState } from '../track';
import { drawTimeline, TimelineStyle } from './timeline-drawing';

// The timeline's drawing is pinned by the calls it makes on a recording context (a digest of them): the same calls
// draw the same pixels.

const STYLE: TimelineStyle = {
  grid: 'gray',
  on: 'green',
  off: 'orange',
  switching: 'silver',
  text: 'white',
  labelBg: 'black',
  death: 'red',
  font: '11px sans-serif',
  strip: 8,
};

/** A run of the given length: on, off and switching in turn, with frames where no bot was seen. */
function run(frameCount: number, fps: number): Timeline {
  const state = new Int8Array(frameCount);
  const outsideDeg = new Float32Array(frameCount);
  for (let i = 0; i < frameCount; i++) {
    state[i] = [
      TrackState.On,
      TrackState.Off,
      TrackState.Off,
      TrackState.Switching,
      TrackState.NoBot,
    ][Math.floor(i / 7) % 5];
    outsideDeg[i] =
      state[i] === TrackState.Off ? (i % 13) / 4 : state[i] === TrackState.On ? 0 : NaN;
  }
  return {
    start: 12,
    frameCount,
    fps,
    state,
    outsideDeg,
    capDeg: 2.5,
    deaths: [40, frameCount - 30],
  };
}

function drawing(timeline: Timeline, widthPx: number, heightPx: number): string {
  const recording = recordingContext(widthPx, heightPx);
  drawTimeline(recording.context, timeline, widthPx, heightPx, STYLE);
  return `${recording.calls.length} ${recording.digest()}`;
}

describe('drawTimeline', () => {
  it('draws a short run: the chart, the strip, the deaths, the labels and every 10 s', () => {
    expect(drawing(run(600, 30), 200, 120)).toBe('1257 5517aeae');
  });

  it('draws a long run every 20 s, with more columns than frames', () => {
    expect([drawing(run(3000, 30), 330, 140), drawing(run(90, 30), 400, 100)]).toEqual([
      '2043 2c70d3fc',
      '2453 f1c064d9',
    ]);
  });
});
