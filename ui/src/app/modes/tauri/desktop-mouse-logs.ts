import { HttpClient, httpResource, HttpResourceRef } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { MouseLoggerState, MouseMeasures } from '../../mouse-api';
import { MouseLogs } from '../../platform/mouse-logs';

/**
 * The desktop app logs the mouse itself (desktop/src/mouse.rs: a logger in a process of its own, switched on and off
 * here) and measures each recording's run from the log in its mouse folder that covers it.
 */
@Service()
export class DesktopMouseLogs implements MouseLogs {
  private readonly http = inject(HttpClient);
  readonly adds = false;
  readonly logs = true;

  measures(id: () => string | undefined): HttpResourceRef<MouseMeasures | null | undefined> {
    return httpResource<MouseMeasures | null>(() => {
      const at = id();
      return at === undefined ? undefined : { url: '/api/mouse', params: { id: at } };
    });
  }

  add(): Promise<MouseMeasures> {
    return Promise.reject(
      new Error('The app finds its own logs: put a log in its mouse folder to have it read'),
    );
  }

  forget(): Promise<void> {
    return Promise.resolve();
  }

  logger(): HttpResourceRef<MouseLoggerState | null | undefined> {
    return httpResource<MouseLoggerState | null>(() => '/api/mouse/logger');
  }

  setLogger(on: boolean): Promise<MouseLoggerState> {
    return firstValueFrom(
      this.http.post<MouseLoggerState>('/api/mouse/logger', null, {
        params: { on: on ? '1' : '0' },
      }),
    );
  }
}
