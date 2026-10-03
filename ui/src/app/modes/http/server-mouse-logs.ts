import { Injectable, resource, ResourceRef } from '@angular/core';
import { MouseLoggerState, MouseMeasures } from '../../mouse-api';
import { MouseLogs } from '../../platform/mouse-logs';

const NOT_HERE =
  'The review server reads no mouse logs: use python/mouse_read.py, the browser or the desktop app';

/** The review server has no route for mouse logs (python/mouse_read.py reads them on the command line): none here. */
@Injectable({ providedIn: 'root' })
export class ServerMouseLogs implements MouseLogs {
  readonly adds = false;
  readonly logs = false;

  measures(id: () => string | undefined): ResourceRef<MouseMeasures | null | undefined> {
    return resource<MouseMeasures | null, string | undefined>({
      params: () => id(),
      loader: () => Promise.resolve(null),
    });
  }

  add(): Promise<MouseMeasures> {
    return Promise.reject(new Error(NOT_HERE));
  }

  forget(): Promise<void> {
    return Promise.resolve();
  }

  logger(): ResourceRef<MouseLoggerState | null | undefined> {
    return resource<MouseLoggerState | null, true>({
      params: () => true,
      loader: () => Promise.resolve(null),
    });
  }

  setLogger(): Promise<MouseLoggerState> {
    return Promise.reject(new Error(NOT_HERE));
  }
}
