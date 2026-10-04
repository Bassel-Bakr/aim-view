// The mouse reader's JSON (mouse-api.ts) numbers each kill and counts each spread's kills as `n`.
/* eslint-disable id-length */
import { MouseKill, MouseRun } from './mouse-api';

/** A kill as the reader measures it, for tests: the first of a run, unless overrides say otherwise. */
export function mouseKill(overrides: Partial<MouseKill>): MouseKill {
  return {
    n: 1,
    kill_local: '04:54:21.304',
    press_local: '04:54:21.339781',
    press_s: 1.339781,
    gap_ms: -0.176,
    shots: 1,
    start_s: 1.1609,
    stop_s: 1.2569,
    settle_s: 1.2569,
    reaction_ms: 162.989,
    flick_ms: 96.005,
    peak_dps: 382.6,
    peak_ms: 208.324,
    stop_to_click_ms: 82.83,
    still_ms: 82.83,
    click_dps: 4.3,
    corrections: 0,
    dist_deg: 21.641,
    ...overrides,
  };
}

/** A run measured from its mouse log, for tests: two kills, the second clicked while moving. */
export function mouseRun(overrides: Partial<MouseRun> = {}): MouseRun {
  return {
    log: {
      wall0: 1790740460,
      start_local: '04:54:20.000000',
      end_local: '04:54:28.523046',
      duration: 8.523,
      events: 17613,
      drift_ms: 0.17,
      devices: [{ handle: 2748, events: 17613 }],
      absolute: 0,
      median_interval: 0.000133,
      busiest_hz: 8010,
      throttled: false,
    },
    scenario: 'Selftest',
    dpi: 1600,
    cm360: 70,
    sens_from: 'the stats file',
    deg_per_count: 0.008164,
    window_ms: 4,
    window_widened: false,
    start_dps: 30,
    stop_dps: 10,
    hold_ms: 5,
    offset_ms: 35.957,
    offset_s: 0.035957,
    kill_count: 2,
    matched: 2,
    gap_p90_ms: 0.7,
    presses_in_run: 3,
    shots: 3,
    misses_s: [2.5],
    kills: [
      mouseKill({}),
      mouseKill({
        n: 2,
        press_local: '04:54:22.001000',
        stop_s: null,
        settle_s: null,
        flick_ms: null,
        stop_to_click_ms: null,
        still_ms: 0,
        click_dps: 135.7,
        corrections: null,
      }),
    ],
    spreads: [
      { key: 'reaction_ms', n: 2, p10: 150, median: 162.989, p90: 170 },
      { key: 'peak_dps', n: 2, p10: 300, median: 382.6, p90: 400 },
      { key: 'still_ms', n: 2, p10: 0, median: 41.4, p90: 80 },
    ],
    moving_clicks: 1,
    no_stop: 1,
    corrected: 0,
    ...overrides,
  };
}
