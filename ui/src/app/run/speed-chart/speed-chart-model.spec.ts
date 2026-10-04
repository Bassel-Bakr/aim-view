import { Flick, PathPoint } from '../../api';
import { fingerprint } from '../recording-context';
import { frameAt, speedChart, xOf, yOf } from './speed-chart-model';

/** A flick at 120 fps: the target closes in fast, then slows and wobbles onto the crosshair. */
const FLICK = {
  kill_number: 4,
  start_frame: 300,
  kill_frame: 372,
  react: 0.15,
  flick: 0.2,
  arrive: 0.42,
  total: 0.6,
} as Flick;
const PATH: PathPoint[] = Array.from({ length: 73 }, (_unused, i): PathPoint => {
  const left = Math.max(0, 14 - i * 0.6) + (i % 4) * 0.03;
  return [300 + i, left, (i % 3) * 0.05];
});

describe('speedChart', () => {
  it('keeps every number of the chart, smoothed or not (a digest of the model as JSON)', () => {
    const models = [false, true].map((smooth) =>
      speedChart(FLICK, PATH, 120, { width: 480, height: 200 }, smooth),
    );
    const places = models.map((model) => [xOf(model, 330), frameAt(model, 250), yOf(model, 75)]);
    expect([...models, places].map((value) => fingerprint(JSON.stringify(value)))).toEqual([
      '13548e11',
      '22f7bedb',
      '68aa67ed',
    ]);
  });
});
