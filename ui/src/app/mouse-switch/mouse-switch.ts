import { Component, computed, inject, signal } from '@angular/core';
import { errorMessage } from '../api';
import { formatNumber } from '../format';
import { MouseLoggerState } from '../mouse-api';
import { MouseLogs } from '../platform/mouse-logs';

/** The switch's line beside it: what the logger is doing or did; failed when it could not. */
export interface LoggerStatus {
  text: string;
  failed: boolean;
}

/** Seconds since 1970 as a local "16:20". */
function clock(seconds: number): string {
  const d = new Date(seconds * 1000);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

/** The line beside the switch, from the logger's state. */
export function loggerStatus(s: MouseLoggerState): LoggerStatus | null {
  if (s.error) return { text: s.error, failed: true };
  if (s.on)
    return { text: `logging since ${s.since == null ? '…' : clock(s.since)}`, failed: false };
  const last = s.last;
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
  protected readonly logs = inject(MouseLogs);
  protected readonly state = this.logs.logger();
  protected readonly busy = signal(false);
  private readonly failure = signal<string | null>(null);

  protected readonly current = computed(() =>
    this.state.hasValue() ? (this.state.value() ?? null) : null,
  );
  protected readonly on = computed(() => this.current()?.on ?? false);
  protected readonly status = computed<LoggerStatus | null>(() => {
    const failed = this.failure();
    if (failed) return { text: failed, failed: true };
    const s = this.current();
    return s ? loggerStatus(s) : null;
  });
  /** What the switch does, where the logs go, and Windows' throttle setting. */
  protected readonly hint = computed(() => {
    const s = this.current();
    const where = s?.folder ? ` into ${s.folder}` : '';
    return `Logs the raw mouse in the background while you play${where}. ${s?.throttle ?? ''}`;
  });

  protected async toggle(): Promise<void> {
    this.busy.set(true);
    this.failure.set(null);
    try {
      const s = await this.logs.setLogger(!this.on());
      this.state.set(s);
      // a logger that cannot start ends at once: look again shortly
      if (s.on) setTimeout(() => this.state.reload(), 1500);
    } catch (e) {
      this.failure.set(errorMessage(e));
    } finally {
      this.busy.set(false);
    }
  }
}
