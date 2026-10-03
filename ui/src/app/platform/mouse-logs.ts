import { ResourceRef } from '@angular/core';
import { MouseLoggerState, MouseMeasures } from '../mouse-api';

/**
 * The raw mouse logs: each recording's run measured from the log that covers it (src/mouse.rs reads it), a log added
 * from this computer, and the logger's switch where the app logs the mouse itself. Each mode provides one
 * (modes/mode.*.ts).
 */
export abstract class MouseLogs {
  /** A log can be added from this computer (the browser keeps it with the recording). */
  abstract readonly adds: boolean;
  /** The app logs the mouse itself while the user plays (the desktop app). */
  abstract readonly logs: boolean;

  /**
   * The recording's run measured from its mouse log; null when it has none (or the mode reads none). Call it where a
   * resource can be made.
   */
  abstract measures(id: () => string | undefined): ResourceRef<MouseMeasures | null | undefined>;

  /** Reads a log from this computer for the recording and keeps it: the run's measures. Rejects a log that does not cover the run. */
  abstract add(id: string, file: File): Promise<MouseMeasures>;

  /** Forgets the log added for the recording. */
  abstract forget(id: string): Promise<void>;

  /** The logger's switch; null where the app does not log. Call it where a resource can be made. */
  abstract logger(): ResourceRef<MouseLoggerState | null | undefined>;

  /** Turns the logger on (a new log) or off. */
  abstract setLogger(on: boolean): Promise<MouseLoggerState>;
}
