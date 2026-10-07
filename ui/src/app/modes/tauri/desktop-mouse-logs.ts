/**
 * The desktop app's `MouseLogs`. In: the app's API (/api/mouse, /api/mouse/logger), which reads
 * the logs in its mouse folder. Out: each recording's run measures for the run page, and the
 * logger's switch for the top bar.
 */

import { HttpClient, httpResource, HttpResourceRef } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { MouseLoggerState, MouseMeasures } from '../../mouse-api';
import { MouseLogs } from '../../platform/mouse-logs';

/**
 * The desktop app logs the mouse itself (desktop/src/mouse.rs: a logger in a process of its own,
 * switched on and off here) and measures each recording's run from the log in its mouse folder
 * that covers it.
 */
@Service()
export class DesktopMouseLogs implements MouseLogs {
  /** Sends the logger's switch. */
  private readonly http = inject(HttpClient);
  /** The app finds its own logs, so the page offers no way to add one. */
  readonly adds = false;
  /** The app logs the mouse itself, so the top bar shows the logger's switch. */
  readonly logs = true;

  /** The run measured from the log in the app's mouse folder that covers it (GET /api/mouse). */
  measures(id: () => string | undefined): HttpResourceRef<MouseMeasures | null | undefined> {
    return httpResource<MouseMeasures | null>(() => {
      const at = id();
      return at === undefined ? undefined : { url: '/api/mouse', params: { id: at } };
    });
  }

  /** Always rejects: the app reads only the logs in its mouse folder. */
  add(): Promise<MouseMeasures> {
    return Promise.reject(
      new Error('The app finds its own logs: put a log in its mouse folder to have it read'),
    );
  }

  /** Does nothing: no log was added from the page, so none is forgotten. */
  forget(): Promise<void> {
    return Promise.resolve();
  }

  /** The logger's state from the app (GET /api/mouse/logger). */
  logger(): HttpResourceRef<MouseLoggerState | null | undefined> {
    return httpResource<MouseLoggerState | null>(() => '/api/mouse/logger');
  }

  /** Starts the logger (a new log) or stops it (POST /api/mouse/logger?on=1 or 0). */
  setLogger(on: boolean): Promise<MouseLoggerState> {
    return firstValueFrom(
      this.http.post<MouseLoggerState>('/api/mouse/logger', null, {
        params: { on: on ? '1' : '0' },
      }),
    );
  }
}
