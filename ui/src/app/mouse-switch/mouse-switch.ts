/**
 * The top bar's mouse logger switch (`MouseSwitch`), where the app logs the mouse itself (the
 * desktop app). In: the MouseLogs contract's logger state. Out: turning the logger on and off,
 * and the line that says what it is doing.
 */

import { Component, computed, inject, signal } from '@angular/core';
import { errorMessage } from '../api';
import { formatNumber } from '../format';
import { MouseLoggerState } from '../mouse-api';
import { MouseLogs } from '../platform/mouse-logs';

/** The switch's line beside it: what the logger is doing or did; failed when it could not. */
export interface LoggerStatus {
  /** The words to show. */
  text: string;
  /** Whether the logger failed, so the line shows as an error. */
  failed: boolean;
}

/** Milliseconds in a second. */
const MS_PER_SECOND = 1000;
/** A logger that cannot start ends at once: the switch looks again this long after starting it. */
const LOOK_AGAIN_MS = 1500;

/** Seconds since 1970 as a local "16:20". */
function clock(seconds: number): string {
  const date = new Date(seconds * MS_PER_SECOND);
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
}

/** The line beside the switch, from the logger's state. */
export function loggerStatus(state: MouseLoggerState): LoggerStatus | null {
  if (state.error) return { text: state.error, failed: true };
  if (state.on)
    return {
      text: `logging since ${state.since == null ? '…' : clock(state.since)}`,
      failed: false,
    };
  const last = state.last;
  if (!last) return null;
  if (last.error) return { text: `last log: ${last.error}`, failed: true };
  const throttled = last.throttled ? ', throttled by Windows' : '';
  return {
    text: `last log: ${formatNumber(last.events ?? 0)} events in ${Math.round(last.duration ?? 0)} s${throttled}`,
    failed: false,
  };
}

/**
 * The desktop app's mouse logger, on and off (desktop/src/mouse.rs): while it is on, the raw mouse is logged in the
 * background as the user plays, and each run a log covers is measured on its page. Shown where the app logs.
 */
@Component({
  selector: 'app-mouse-switch',
  templateUrl: './mouse-switch.html',
  styleUrl: './mouse-switch.scss',
})
export class MouseSwitch {
  /** The mode's mouse logs and logger. */
  protected readonly logs = inject(MouseLogs);
  /** The logger's state; null where the app does not log. */
  protected readonly state = this.logs.logger();
  /** Whether the switch is being turned. */
  protected readonly busy = signal(false);
  /** Why the last turn of the switch failed; null when it did not. */
  private readonly failure = signal<string | null>(null);

  /** The logger's state once it has loaded; null before, or where the app does not log. */
  protected readonly current = computed(() =>
    this.state.hasValue() ? (this.state.value() ?? null) : null,
  );
  /** Whether the logger is running. */
  protected readonly on = computed(() => this.current()?.on ?? false);
  /** The line beside the switch: a failed turn, else what the logger says. */
  protected readonly status = computed<LoggerStatus | null>(() => {
    const failed = this.failure();
    if (failed) return { text: failed, failed: true };
    const state = this.current();
    return state ? loggerStatus(state) : null;
  });
  /** What the switch does, where the logs go, and Windows' throttle setting. */
  protected readonly hint = computed(() => {
    const state = this.current();
    const where = state?.folder ? ` into ${state.folder}` : '';
    return `Logs the raw mouse in the background while you play${where}. ${state?.throttle ?? ''}`;
  });

  /** Turns the logger on or off; a start is checked again LOOK_AGAIN_MS later. */
  protected async toggle(): Promise<void> {
    this.busy.set(true);
    this.failure.set(null);
    try {
      const state = await this.logs.setLogger(!this.on());
      this.state.set(state);
      if (state.on) setTimeout(() => this.state.reload(), LOOK_AGAIN_MS);
    } catch (error) {
      this.failure.set(errorMessage(error));
    } finally {
      this.busy.set(false);
    }
  }
}
