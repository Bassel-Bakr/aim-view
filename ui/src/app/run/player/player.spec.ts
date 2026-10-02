import { ClickReport, TrackReport } from '../../api';
import { markPositions } from './player';

describe('markPositions', () => {
  it('marks each kill of a clicking run, as a share of the video', () => {
    const report = {
      mode: 'click',
      fps: 100,
      flicks: [{ kill_frame: 500 }, { kill_frame: 1500 }],
    } as unknown as ClickReport;
    expect(markPositions(report, 20)).toEqual([25, 75]);
  });

  it("marks each bot's death in a tracking run", () => {
    const report = {
      mode: 'track',
      fps: 100,
      summary: { switches: [[1000, 1100]] },
    } as unknown as TrackReport;
    expect(markPositions(report, 20)).toEqual([50]);
  });

  it('marks nothing before the video has a length', () => {
    expect(
      markPositions({ mode: 'click', fps: 100, flicks: [] } as unknown as ClickReport, 0),
    ).toEqual([]);
  });
});
