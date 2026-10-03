import { Recording } from '../api';
import { scoreChange } from './score-change';

const run = (scenario: string, score: number | null, stamp: string) =>
  ({ id: `${scenario} ${stamp}`, scenario, score, stamp }) as Recording;

describe('scoreChange', () => {
  const now = run('1wall', 558.46, '2026.10.01-16.23.04');

  it("compares with the same scenario's latest run before", () => {
    const all = [
      now,
      run('1wall', 467.53, '2026.10.01-16.21.38'),
      run('1wall', 400, '2026.09.30-10.00.00'),
      run('1wall', 900, '2026.10.02-10.00.00'),
      run('other', 100, '2026.10.01-16.22.00'),
    ];
    expect(scoreChange(now, all)).toEqual({ text: '+90.93 on the run before', up: true });
  });

  it('says a drop, and nothing without a run before', () => {
    expect(scoreChange(now, [now, run('1wall', 600, '2026.10.01-16.00.00')])).toEqual({
      text: '−41.54 on the run before',
      up: false,
    });
    expect(scoreChange(now, [now, run('1wall', null, '2026.10.01-16.00.00')])).toBeNull();
    expect(scoreChange(run('1wall', null, '2026.10.01-16.23.04'), [now])).toBeNull();
  });
});
