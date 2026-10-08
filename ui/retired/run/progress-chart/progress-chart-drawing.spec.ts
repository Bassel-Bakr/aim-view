import { PastRun } from '../../platform/score-history';
import { recordingContext } from '../recording-context';
import { drawProgressChart, HistoryStyle } from './progress-chart-drawing';
import { HistoryModel, progressChart } from './progress-chart-model';

// The chart's drawing is pinned by the calls it makes on a recording context (a digest of them): the same calls draw
// the same pixels.

const STYLE: HistoryStyle = {
  run: 'gray',
  median: 'blue',
  best: 'gold',
  current: 'red',
  grid: 'silver',
  label: 'white',
  surface: 'black',
  font: '11px sans-serif',
  dot: 2.5,
  currentDot: 4,
  ring: 6,
  reach: 8,
  line: 2,
  dash: 3,
};

/** A run a day for the given days from 1 September, two a day, their scores rising and falling. */
function runs(days: number): PastRun[] {
  return Array.from({ length: days * 2 }, (_unused, i) => ({
    stamp: `2026.09.${String(1 + Math.floor(i / 2)).padStart(2, '0')}-1${i % 2}.00.00`,
    score: 500 + ((i * 37) % 90),
    kills: null,
    accuracy: null,
  }));
}

function drawing(model: HistoryModel | null): string {
  if (!model) throw new Error('no model');
  const recording = recordingContext(model.size.width, model.size.height);
  drawProgressChart(recording.context, model, STYLE);
  return `${recording.calls.length} ${recording.digest()}`;
}

describe('drawProgressChart', () => {
  it('draws the grid, the dates, the best, the dots, the median and this run', () => {
    expect(
      drawing(progressChart(runs(5), '2026.09.03-11.00.00', { width: 600, height: 200 })),
    ).toBe('115 c5b03573');
  });

  it('draws a chart without this run, and a narrow one with dates left out', () => {
    expect([
      drawing(progressChart(runs(5), null, { width: 600, height: 200 })),
      drawing(progressChart(runs(20), null, { width: 300, height: 160 })),
    ]).toEqual(['109 dea4e79a', '228 137d66a1']);
  });
});
