import { HttpClient, httpResource, HttpResourceRef } from '@angular/common/http';
import { inject, Service, resource, ResourceRef } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { MouseLoggerState, MouseMeasures } from '../../mouse-api';
import { MouseLogs } from '../../platform/mouse-logs';
import { MountedFiles } from './mounted-files';

/** Where the review service keeps the mouse logs (its data folder's mouse/). */
const MOUSE = '/data/mouse';

/**
 * Browser mode's mouse logs: a log the user adds is kept by the review service in its mouse folder (POST
 * /api/mouse_log), which measures each recording's run from the log that covers it (/api/mouse), as the desktop app
 * does with the logs it writes (DesktopMouseLogs). The browser cannot log the mouse itself.
 */
@Service()
export class BrowserMouseLogs implements MouseLogs {
  private readonly http = inject(HttpClient);
  private readonly files = inject(MountedFiles);
  readonly adds = true;
  readonly logs = false;

  measures(id: () => string | undefined): HttpResourceRef<MouseMeasures | null | undefined> {
    return httpResource<MouseMeasures | null>(() => {
      const at = id();
      return at === undefined ? undefined : { url: '/api/mouse', params: { id: at } };
    });
  }

  /** Kept by the service, then the run measured from it; a log that does not cover the run is turned down. */
  async add(id: string, file: File): Promise<MouseMeasures> {
    await firstValueFrom(this.http.post('/api/mouse_log', file, { params: { name: file.name } }));
    const measured = await this.measured(id);
    if (!measured?.run)
      throw new Error(`${file.name}: ${measured?.error ?? 'the log does not cover this run'}`);
    return measured;
  }

  /** Removes the copy of the log that measures the recording from the service's mouse folder. */
  async forget(id: string): Promise<void> {
    const measured = await this.measured(id);
    if (measured?.file) await this.files.remove(`${MOUSE}/${measured.file}`);
  }

  logger(): ResourceRef<MouseLoggerState | null | undefined> {
    return resource<MouseLoggerState | null, true>({
      params: () => true,
      loader: () => Promise.resolve(null),
    });
  }

  setLogger(): Promise<MouseLoggerState> {
    return Promise.reject(
      new Error('The browser cannot log the mouse: use the desktop app or python/mouse_log.py'),
    );
  }

  private measured(id: string): Promise<MouseMeasures | null> {
    return firstValueFrom(this.http.get<MouseMeasures | null>('/api/mouse', { params: { id } }));
  }
}
