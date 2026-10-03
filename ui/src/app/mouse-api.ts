/**
 * The raw mouse log's measures and the logger's switch, typed: what the core's reader gives (src/mouse.rs, the same
 * as python/mouse_read.py) and what the desktop app answers (desktop/src/mouse.rs).
 */

/** A device in the log: its handle and its events. */
export interface MouseDevice {
  handle: number;
  events: number;
}

/** What any log says: its span (local times), events, clock drift, devices and event rate. */
export interface MouseLogFacts {
  /** The start's wall time, seconds since 1970. */
  wall0: number;
  start_local: string;
  end_local: string;
  duration: number;
  events: number;
  /** The wall clock against the logger's clock over the log, ms; null when the logger was killed. */
  drift_ms: number | null;
  devices: MouseDevice[];
  /** Absolute events (a tablet or a remote desktop), left out. */
  absolute: number;
  /** The median time between events, s; null with fewer than two. */
  median_interval: number | null;
  /** Events a second in the busiest 100 ms. */
  busiest_hz: number;
  /** The events came about 8 ms apart: Windows probably throttled the logger. */
  throttled: boolean;
}

/** One kill's measures, from the previous kill's click to its own (times in ms, speeds in degrees a second). */
export interface MouseKill {
  /** The kill's number in the stats file. */
  n: number;
  kill_local: string;
  press_local: string;
  /** The click, seconds since the log started. */
  press_s: number;
  /** The click minus the stats file's kill time, ms. */
  gap_ms: number;
  shots: number;
  start_s: number | null;
  stop_s: number | null;
  settle_s: number | null;
  reaction_ms: number | null;
  flick_ms: number | null;
  peak_dps: number;
  peak_ms: number;
  stop_to_click_ms: number | null;
  still_ms: number;
  click_dps: number;
  corrections: number | null;
  /** How far the crosshair moved from the previous click, degrees. */
  dist_deg: number;
}

/** The measures the reader sums up over the kills. */
export type MouseMeasureKey =
  | 'reaction_ms'
  | 'flick_ms'
  | 'peak_dps'
  | 'stop_to_click_ms'
  | 'still_ms'
  | 'click_dps'
  | 'dist_deg';

/** One measure over the kills that have it: how many, and its p10, median and p90. */
export interface MouseSpread {
  key: MouseMeasureKey;
  n: number;
  p10: number;
  median: number;
  p90: number;
}

/** A run measured from its mouse log: everything python/mouse_read.py prints, and each kill. */
export interface MouseRun {
  log: MouseLogFacts;
  scenario: string;
  dpi: number;
  cm360: number;
  /** Where the sensitivity came from: "options", "the stats file" or "defaults". */
  sens_from: string;
  deg_per_count: number;
  /** The speed window, ms; widened to twice the median interval when the events are far apart. */
  window_ms: number;
  window_widened: boolean;
  start_dps: number;
  stop_dps: number;
  hold_ms: number;
  /** The click minus the stats file's kill time, ms. */
  offset_ms: number;
  offset_s: number;
  kill_count: number;
  /** Kills matched with a click within 10 ms. */
  matched: number;
  gap_p90_ms: number;
  presses_in_run: number;
  /** The shots in the stats file. */
  shots: number;
  /** Clicks in the run that killed nothing, s since the log started. */
  misses_s: number[];
  kills: MouseKill[];
  spreads: MouseSpread[];
  moving_clicks: number;
  no_stop: number;
  corrected: number;
}

/** A log on its own, without its run's stats file: its facts, the total travel and the clicks. */
export interface MouseLogSummary {
  log: MouseLogFacts;
  dpi: number;
  cm360: number;
  deg_per_count: number;
  travel_deg: number;
  presses: number;
}

/** What the reader is asked: the run's stats file (name and text), and the time zone (local minus UTC, s). */
export interface MouseReadRequest {
  stats_name: string | null;
  stats_text: string | null;
  utc_offset: number;
}

/** The reader's answer: the run measured. */
export interface MouseReadRun {
  run: MouseRun;
}

/** The reader's answer: the log on its own. */
export interface MouseReadSummary {
  summary: MouseLogSummary;
}

/** The reader's answer: why it measured nothing. */
export interface MouseReadError {
  error: string;
}

export type MouseReadOutcome = MouseReadRun | MouseReadSummary | MouseReadError;

/** A recording's run measured from its mouse log: the log's file, and the run's measures or why there are none. */
export interface MouseMeasures {
  file: string | null;
  run: MouseRun | null;
  error: string | null;
}

/** The last log the logger wrote: its file, events, span and rates, or why it cannot be read. */
export interface MouseLogged {
  file: string | null;
  events?: number;
  duration?: number;
  busiest_hz?: number;
  median_interval?: number | null;
  throttled?: boolean;
  error?: string;
}

/** The logger's switch (the desktop app): on or off, its file, where logs go, the last log and Windows' throttle. */
export interface MouseLoggerState {
  available: boolean;
  on: boolean;
  file: string | null;
  /** When it started, seconds since 1970. */
  since: number | null;
  folder: string | null;
  /** The background raw input throttle setting, in words. */
  throttle: string;
  last: MouseLogged | null;
  error: string | null;
}
