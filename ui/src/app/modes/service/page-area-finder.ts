import { HttpClient, HttpErrorResponse } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { FinderReply, FinderWork } from '../wasm/area-finder-messages';
import { MountedFiles } from './mounted-files';

/** The service's answer when it has no found areas for a recording: the page's finder must read its video first. */
export interface NeedFound {
  error: string;
  need: 'found';
  video: string;
}

/** The service's 409 asking for the area finder's reading of the video; null for any other failure. */
export function needsFound(e: unknown): NeedFound | null {
  const body =
    e instanceof HttpErrorResponse && e.status === 409 ? (e.error as NeedFound | null) : null;
  return body?.need === 'found' ? body : null;
}

/** What the area finder found in the video (src/areas.rs: Found, as JSON), read in a worker (area-finder.worker.ts). */
function readFrames(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL('../wasm/area-finder.worker', import.meta.url), {
      type: 'module',
    });
    worker.onmessage = (e: MessageEvent<FinderReply>) => {
      worker.terminate();
      if (e.data.kind === 'error') reject(new Error(e.data.error));
      else resolve(e.data.found);
    };
    worker.onerror = (e) => {
      worker.terminate();
      reject(new Error(e.message));
    };
    const work: FinderWork = { file, coreUrl: new URL('core/aimview.wasm', document.baseURI).href };
    worker.postMessage(work);
  });
}

/**
 * The area finder (src/areas.rs) in the page, for the review service: it reads a recording's frames in a worker of its
 * own when the service has no found areas for it (its 409, need: found), and sends what it found (POST /api/found),
 * which the service keeps. Once a recording: after its review, or when Find areas needs it.
 */
@Service()
export class PageAreaFinder {
  private readonly http = inject(HttpClient);
  private readonly files = inject(MountedFiles);
  /** The readings under way, by recording, so one runs for every call meanwhile. */
  private readonly reading = new Map<string, Promise<void>>();

  /** Has the finder read the recording when the service has nothing kept for it (after a review). */
  async ensure(id: string): Promise<void> {
    const params = { id, copy: '0' };
    try {
      await firstValueFrom(this.http.get('/api/find_areas', { params }));
    } catch (e) {
      const need = needsFound(e);
      if (!need) throw e;
      await this.find(id, need.video);
    }
  }

  /** Reads the video's frames (a mounted path) and sends what the finder found for the recording. */
  find(id: string, video: string): Promise<void> {
    let reading = this.reading.get(id);
    if (!reading) {
      reading = this.read(id, video).finally(() => this.reading.delete(id));
      this.reading.set(id, reading);
    }
    return reading;
  }

  private async read(id: string, video: string): Promise<void> {
    const found = await readFrames(await this.files.read(video));
    await firstValueFrom(this.http.post('/api/found', found, { params: { id } }));
  }
}
