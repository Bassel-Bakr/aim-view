import { ClickReport, Flick } from '../../api';
import { recordingContext } from '../recording-context';
import { drawKillLanes, LaneStyle } from './kill-lanes-drawing';

// The lanes' drawing is pinned by the calls it makes on a recording context (a digest of them): the same calls draw
// the same pixels.

const STYLE: LaneStyle = {
  kill: 'teal',
  picked: 'white',
  quiet: 'gray',
  long: 'coral',
  grid: 'silver',
  markHeight: 10,
  barWidth: 3,
  split: 16,
};

/** Twenty kills at 60 fps, their times from quick to past the tallest bar. */
const REPORT = {
  mode: 'click',
  fps: 60,
  flicks: Array.from(
    { length: 20 },
    (_unused, i) =>
      ({ kill_number: i + 1, kill_frame: 90 * i + 45, total: 0.2 + (i % 7) * 0.4 }) as Flick,
  ),
} as unknown as ClickReport;

function drawing(picked: Flick | null): string {
  const recording = recordingContext(600, 60);
  drawKillLanes(recording.context, REPORT, 600, 60, 32, picked, STYLE);
  return `${recording.calls.length} ${recording.digest()}`;
}

describe('drawKillLanes', () => {
  it("draws each kill's mark and its time as a bar, long ones in the attention color, the picked one apart", () => {
    expect([drawing(null), drawing(REPORT.flicks[4])]).toEqual(['82 734889f8', '82 37734331']);
  });
});
