/**
 * The raw mouse log's measures and the logger's switch, typed: what the core's reader gives (src/mouse.rs, the same
 * as python/mouse_read.py) and what the desktop app answers (desktop/src/mouse.rs).
 * Out: the MouseLogs contract (platform/mouse-logs.ts), the run page's mouse panel and the top
 * bar's logger switch (mouse-switch/).
 */

/** A device in the log: its handle and its events. */
export interface MouseDevice {
  /** Windows' raw input handle for the device. */
  handle: number;
  /** How many events the device sent. */
  events: number;
}

/** What any log says: its span (local times), events, clock drift, devices and event rate. */
export interface MouseLogFacts {
  /** The start's wall time, seconds since 1970. */
  wall0: number;
  /** When the log started, local time ("%H:%M:%S.%f"). */
  start_local: string;
  /** When the log ended, local time ("%H:%M:%S.%f"). */
  end_local: string;
  /** How long the log runs, in seconds. */
  duration: number;
  /** How many events it holds. */
  events: number;
  /** The wall clock against the logger's clock over the log, ms; null when the logger was killed. */
  drift_ms: number | null;
  /** The devices that sent events, in the log's order. */
  devices: MouseDevice[];
  /** Absolute events (a tablet or a remote desktop), left out. */
  absolute: number;
  /** The median time between events, s; null with fewer than two. */
  median_interval: number | null;
  /** Events a second in the busiest 100 ms. */
  busiest_hz: number;
  /**
   * The events came far apart (a median over 2 ms, in 200 events or more): Windows probably
   * throttled the logger.
   */
  throttled: boolean;
}

/** One kill's measures, from the previous kill's click to its own (times in ms, speeds in degrees a second). */
export interface MouseKill {
  /** The kill's number in the stats file. */
  n: number;
  /** The kill's time in the stats file, local ("%H:%M:%S.%f"). */
  kill_local: string;
  /** The click's time, local ("%H:%M:%S.%f"). */
  press_local: string;
  /** The click, seconds since the log started. */
  press_s: number;
  /** The click minus the stats file's kill time, ms. */
  gap_ms: number;
  /** The shots the stats file gives the kill. */
  shots: number;
  /** When the mouse started moving, seconds since the log started; null when it never did. */
  start_s: number | null;
  /** When it stopped (under the stop speed), seconds since the log started; null if it did not. */
  stop_s: number | null;
  /** When the last still stretch before the click began, seconds since the log started. */
  settle_s: number | null;
  /** From the previous click until the mouse starts moving, ms. */
  reaction_ms: number | null;
  /** From the start until the mouse stops, ms. */
  flick_ms: number | null;
  /** The highest speed, in degrees a second. */
  peak_dps: number;
  /** When the speed peaked, ms after the previous click. */
  peak_ms: number;
  /** From the stop to the click, ms. */
  stop_to_click_ms: number | null;
  /** How long the mouse stayed still right before the click, ms; 0 when it clicked while moving. */
  still_ms: number;
  /** The speed at the click, in degrees a second. */
  click_dps: number;
  /** How many times the speed rose to the stop speed again after the stop. */
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
  /** Which measure it is. */
  key: MouseMeasureKey;
  /** How many kills have the measure. */
  n: number;
  /** The tenth percentile, in the measure's unit. */
  p10: number;
  /** The median, in the measure's unit. */
  median: number;
  /** The ninetieth percentile, in the measure's unit. */
  p90: number;
}

/** A run measured from its mouse log: everything python/mouse_read.py prints, and each kill. */
export interface MouseRun {
  /** What the log says on its own. */
  log: MouseLogFacts;
  /** The scenario, from the stats file. */
  scenario: string;
  /** The mouse's dots per inch. */
  dpi: number;
  /** How many centimeters the mouse moves for a full turn. */
  cm360: number;
  /** Where the sensitivity came from: "options", "the stats file" or "defaults". */
  sens_from: string;
  /** Degrees the view turns for one count of the mouse. */
  deg_per_count: number;
  /** The speed window, ms; widened to twice the median interval when the events are far apart. */
  window_ms: number;
  /** Whether the speed window was widened. */
  window_widened: boolean;
  /** A flick starts at this speed, in degrees a second. */
  start_dps: number;
  /** A flick stops under this speed, in degrees a second. */
  stop_dps: number;
  /** How long a stop holds under the stop speed, ms. */
  hold_ms: number;
  /** The click minus the stats file's kill time, ms. */
  offset_ms: number;
  /** The same offset in seconds, unrounded. */
  offset_s: number;
  /** How many kills the stats file has. */
  kill_count: number;
  /** Kills matched with a click within 10 ms. */
  matched: number;
  /** The ninetieth percentile of the matched kills' gaps between click and kill, ms. */
  gap_p90_ms: number;
  /** How many clicks the log has between the run's start and end. */
  presses_in_run: number;
  /** The shots in the stats file. */
  shots: number;
  /** Clicks in the run that killed nothing, s since the log started. */
  misses_s: number[];
  /** Each kill's measures, in the stats file's order. */
  kills: MouseKill[];
  /** Each measure's spread over the kills, for the measures some kill has. */
  spreads: MouseSpread[];
  /** Kills clicked while the mouse was moving. */
  moving_clicks: number;
  /** Kills with no stop before the click. */
  no_stop: number;
  /** Kills with a correction after the stop. */
  corrected: number;
}

/** A log on its own, without its run's stats file: its facts, the total travel and the clicks. */
export interface MouseLogSummary {
  /** What the log says. */
  log: MouseLogFacts;
  /** The mouse's dots per inch (the defaults, without a stats file). */
  dpi: number;
  /** How many centimeters the mouse moves for a full turn. */
  cm360: number;
  /** Degrees the view turns for one count of the mouse. */
  deg_per_count: number;
  /** How far the mouse moved over the whole log, in degrees. */
  travel_deg: number;
  /** How many left-button clicks the log has. */
  presses: number;
}

/** What the reader is asked: the run's stats file (name and text), and the time zone (local minus UTC, s). */
export interface MouseReadRequest {
  /** The stats file's name, which gives the run's date; null for the log on its own. */
  stats_name: string | null;
  /** The stats file's text; null for the log on its own. */
  stats_text: string | null;
  /** Local time minus UTC, in seconds. */
  utc_offset: number;
}

/** The reader's answer: the run measured. */
export interface MouseReadRun {
  /** The run's measures. */
  run: MouseRun;
}

/** The reader's answer: the log on its own. */
export interface MouseReadSummary {
  /** The log's summary. */
  summary: MouseLogSummary;
}

/** The reader's answer: why it measured nothing. */
export interface MouseReadError {
  /** What went wrong, in the reader's words. */
  error: string;
}

/** What the reader gives: the run measured, the log on its own, or why it measured nothing. */
export type MouseReadOutcome = MouseReadRun | MouseReadSummary | MouseReadError;

/** A recording's run measured from its mouse log: the log's file, and the run's measures or why there are none. */
export interface MouseMeasures {
  /** The log's file name; null when the recording has no log. */
  file: string | null;
  /** The run's measures; null when there are none. */
  run: MouseRun | null;
  /** Why there are no measures; null when there are. */
  error: string | null;
}

/** The last log the logger wrote: its file, events, span and rates, or why it cannot be read. */
export interface MouseLogged {
  /** The log's file name. */
  file: string | null;
  /** How many events it holds. */
  events?: number;
  /** How long it runs, in seconds. */
  duration?: number;
  /** Events a second in the busiest 100 ms. */
  busiest_hz?: number;
  /** The median time between events, s. */
  median_interval?: number | null;
  /** Whether Windows probably throttled the logger (events far apart, or a low busiest rate). */
  throttled?: boolean;
  /** Why the log cannot be read. */
  error?: string;
}

/** The logger's switch (the desktop app): on or off, its file, where logs go, the last log and Windows' throttle. */
export interface MouseLoggerState {
  /** Whether this machine can log the mouse (Windows only). */
  available: boolean;
  /** Whether the logger is running. */
  on: boolean;
  /** The running log's file name; null when it is off. */
  file: string | null;
  /** When it started, seconds since 1970. */
  since: number | null;
  /** The folder the logs go to. */
  folder: string | null;
  /** The background raw input throttle setting, in words. */
  throttle: string;
  /** The last log it wrote; null before the first. */
  last: MouseLogged | null;
  /** Why the logger stopped or failed; null when it did not. */
  error: string | null;
}
