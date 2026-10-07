/**
 * The area finder for browser mode's service. In: the service in the page's 409 asking for a
 * recording's found areas, and the recording's video from its mounts. Out: the areas the finder
 * found (area-finder.worker.ts), sent to the service (POST /api/found), which keeps them.
 */

import { HttpClient, HttpErrorResponse } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { FinderReply, FinderWork } from '../wasm/area-finder-messages';
import { MountedFiles } from './mounted-files';

/** The status the service asks for the area finder's reading with. */
const HTTP_CONFLICT = 409;

/**
 * The service's answer when it has no found areas for a recording: the page's finder must read its
 * video first.
 */
export interface NeedFound {
  /** The service's message, in words. */
  error: string;
  /** What the service needs: the found areas. */
  need: 'found';
  /** The recording's video, as a mounted path. */
  video: string;
}

/**
 * The service's 409 asking for the area finder's reading of the video; null for any other
 * failure.
 */
export function needsFound(error: unknown): NeedFound | null {
  const body =
    error instanceof HttpErrorResponse && error.status === HTTP_CONFLICT
      ? (error.error as NeedFound | null)
      : null;
  return body?.need === 'found' ? body : null;
}

/**
 * What the area finder found in the video (src/areas.rs: Found, as JSON), read in a worker
 * (area-finder.worker.ts). Rejects with the worker's error.
 */
function readFrames(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL('../wasm/area-finder.worker', import.meta.url), {
      type: 'module',
    });
    worker.onmessage = (event: MessageEvent<FinderReply>) => {
      worker.terminate();
      if (event.data.kind === 'error') reject(new Error(event.data.error));
      else resolve(event.data.found);
    };
    worker.onerror = (event) => {
      worker.terminate();
      reject(new Error(event.message));
    };
    const work: FinderWork = { file, coreUrl: new URL('core/aimview.wasm', document.baseURI).href };
    worker.postMessage(work);
  });
}

/**
 * The area finder (src/areas.rs) in the page, for the review service: it reads a recording's
 * frames in a worker of its own when the service has no found areas for it (its 409, need: found),
 * and sends what it found (POST /api/found), which the service keeps. Once a recording: after its
 * review, or when Find areas needs it.
 */
@Service()
export class PageAreaFinder {
  /** Asks the service and sends it the areas found. */
  private readonly http = inject(HttpClient);
  /** Reads the recording's video from the service's mounts. */
  private readonly files = inject(MountedFiles);
  /** The readings under way, by recording, so one runs for every call meanwhile. */
  private readonly reading = new Map<string, Promise<void>>();

  /**
   * Has the finder read the recording when the service has nothing kept for it (after a review):
   * GET /api/find_areas answers 409 then.
   */
  async ensure(id: string): Promise<void> {
    const params = { id, copy: '0' };
    try {
      await firstValueFrom(this.http.get('/api/find_areas', { params }));
    } catch (error) {
      const need = needsFound(error);
      if (!need) throw error;
      await this.find(id, need.video);
    }
  }

  /**
   * Reads the video's frames (a mounted path) and sends what the finder found for the recording;
   * a second call while one reads gets the same promise.
   */
  find(id: string, video: string): Promise<void> {
    let reading = this.reading.get(id);
    if (!reading) {
      reading = this.read(id, video).finally(() => this.reading.delete(id));
      this.reading.set(id, reading);
    }
    return reading;
  }

  /** Reads the video in a worker, then sends the areas found to the service. */
  private async read(id: string, video: string): Promise<void> {
    const found = await readFrames(await this.files.read(video));
    await firstValueFrom(this.http.post('/api/found', found, { params: { id } }));
  }
}
