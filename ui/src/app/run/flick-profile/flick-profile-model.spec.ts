import { Flick, FlickProfile } from '../../api';
import { killCurve, PROFILE_BOX, profileChart } from './flick-profile-model';

/** A profile of 0 to 125% in 6 points, 25% apart, rising to its top at 50%. */
const profile: FlickProfile = {
  flicks: 12,
  step: 0.25,
  mean: [0.4, 0.8, 1, 0.6, 0.1, 0],
  p25: [0.3, 0.7, 0.9, 0.5, 0.05, 0],
  p75: [0.5, 0.9, 1, 0.7, 0.2, 0.05],
  peak_at: 0.5,
  braking: 0.375,
};

const plotRight = PROFILE_BOX.width - PROFILE_BOX.right;
const plotBottom = PROFILE_BOX.height - PROFILE_BOX.bottom;

describe('flick profile chart', () => {
  it('has no chart without a profile', () => {
    expect(profileChart(undefined)).toBeNull();
    expect(profileChart(null)).toBeNull();
  });

  it('spans the profile, marks the flick end and the top speed, and says both numbers', () => {
    const chart = profileChart(profile)!;
    expect(chart.span).toBe(1.25);
    expect(chart.xTicks.map((tick) => tick.label)).toEqual([
      '0%',
      '25%',
      '50%',
      '75%',
      '100%',
      '125%',
    ]);
    expect(chart.xTicks[0].at).toBe(PROFILE_BOX.left);
    expect(chart.xTicks[5].at).toBeCloseTo(plotRight, 6);
    expect(chart.end).toBeCloseTo(chart.xTicks[4].at, 6);
    // the top speed sits on the average at 50%, its highest point
    expect(chart.peak.x).toBeCloseTo(chart.xTicks[2].at, 6);
    expect(chart.peak.y).toBeCloseTo(chart.yTicks[4].at, 6);
    expect(chart.yTicks[0].at).toBe(plotBottom);
    expect(chart.summary).toBe('Flick speed tops out at 50% of the flick; braking takes 38%.');
    // the band goes out along the 75th percentile and back along the 25th, closed
    expect(chart.band.match(/[ML]/g)).toHaveLength(12);
    expect(chart.band.endsWith('Z')).toBe(true);
    expect(chart.mean.startsWith(`M${PROFILE_BOX.left.toFixed(1)},`)).toBe(true);
  });

  it("draws a kill's own curve as shares of its top speed, cut at the plot's edge", () => {
    const chart = profileChart(profile)!;
    const kill = {
      kill_number: 3,
      speed: { speeds: [100, 300, 400, 200, 50, 20, 10], flick_end: 4 },
    } as Flick;
    const curve = killCurve(kill, chart)!;
    const points = curve
      .slice(1)
      .split('L')
      .map((point) => point.split(',').map(Number));
    // 0 to 125% of a 4-frame flick: 6 frames, the seventh (150%) left out
    expect(points).toHaveLength(6);
    expect(points[0][0]).toBeCloseTo(PROFILE_BOX.left, 1);
    expect(points[4][0]).toBeCloseTo(chart.end, 1);
    // its top (400 °/s, at 50%) on the 100% line
    expect(points[2][1]).toBeCloseTo(chart.yTicks[4].at, 1);
    expect(killCurve(null, chart)).toBeNull();
    expect(killCurve({ kill_number: 4 } as Flick, chart)).toBeNull();
  });
});
