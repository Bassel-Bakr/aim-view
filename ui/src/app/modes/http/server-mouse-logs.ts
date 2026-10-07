/**
 * Server mode's `MouseLogs`, which offers none. In: nothing. Out: empty measures and no logger, so
 * the run page and the top bar hide their mouse parts.
 */

import { Service, resource, ResourceRef } from '@angular/core';
import { MouseLoggerState, MouseMeasures } from '../../mouse-api';
import { MouseLogs } from '../../platform/mouse-logs';

/** Why adding a log or switching the logger fails in this mode, and where to go instead. */
const NOT_HERE =
  'The review server reads no mouse logs: use python/mouse_read.py, the browser or the desktop app';

/**
 * The review server has no route for mouse logs (python/mouse_read.py reads them on the command
 * line): none here.
 */
@Service()
export class ServerMouseLogs implements MouseLogs {
  /** No log can be added here. */
  readonly adds = false;
  /** The server does not log the mouse. */
  readonly logs = false;

  /** Always null: the server has no logs to measure from. */
  measures(id: () => string | undefined): ResourceRef<MouseMeasures | null | undefined> {
    return resource<MouseMeasures | null, string | undefined>({
      params: () => id(),
      loader: () => Promise.resolve(null),
    });
  }

  /** Always rejects with `NOT_HERE`. */
  add(): Promise<MouseMeasures> {
    return Promise.reject(new Error(NOT_HERE));
  }

  /** Does nothing: no log was added. */
  forget(): Promise<void> {
    return Promise.resolve();
  }

  /** Always null: the server has no logger. */
  logger(): ResourceRef<MouseLoggerState | null | undefined> {
    return resource<MouseLoggerState | null, true>({
      params: () => true,
      loader: () => Promise.resolve(null),
    });
  }

  /** Always rejects with `NOT_HERE`. */
  setLogger(): Promise<MouseLoggerState> {
    return Promise.reject(new Error(NOT_HERE));
  }
}
