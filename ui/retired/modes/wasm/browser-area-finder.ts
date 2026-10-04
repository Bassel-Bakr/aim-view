import { inject, Injectable } from '@angular/core';
import { BrowserStore } from '../web-files/browser-store';
import { fingerprint, SavedReviews } from '../web-files/saved-reviews';
import { FinderReply, FinderResult, FinderWork } from './area-finder-messages';

const KEY = 'area-finder:';

/** What the area finder found in the file, read in a worker (area-finder.worker.ts). */
function readFrames(file: File): Promise<FinderResult> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL('./area-finder.worker', import.meta.url), { type: 'module' });
    worker.onmessage = (e: MessageEvent<FinderReply>) => {
      worker.terminate();
      if (e.data.kind === 'error') reject(new Error(e.data.error));
      else resolve(JSON.parse(e.data.found) as FinderResult);
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
 * The area finder in the browser (src/areas.rs): it reads a recording's frames in a worker of its own, once a
 * recording: after the recording's review, or without one when Find areas needs it. What it found is kept in this
 * browser (IndexedDB) for the file, as its reviews are, so the same file added again finds it.
 */
@Injectable({ providedIn: 'root' })
export class BrowserAreaFinder {
  private readonly store = inject(BrowserStore);
  private readonly reviews = inject(SavedReviews);
  /** What the finder found, or is finding, by the file's fingerprint. */
  private readonly results = new Map<string, Promise<FinderResult>>();

  /** What the finder found in the file: kept, else read now (one reading for every call meanwhile). */
  result(file: File): Promise<FinderResult> {
    const key = fingerprint(file);
    let found = this.results.get(key);
    if (!found) {
      found = this.kept(file).then((kept) => kept ?? this.read(file));
      this.results.set(key, found);
      // a reading that failed is tried again on the next call
      found.catch(() => this.results.delete(key));
    }
    return found;
  }

  /** What the finder found in the file, or is finding; null when it has not read it (this does not start it). */
  known(file: File): Promise<FinderResult | null> {
    return this.results.get(fingerprint(file))?.catch(() => null) ?? this.kept(file);
  }

  /** As kept in this browser, else as a review saved before the finder had a worker of its own kept it. */
  private async kept(file: File): Promise<FinderResult | null> {
    const kept = await this.store.get<FinderResult>(KEY + fingerprint(file));
    return kept ?? (await this.reviews.finderResult(file));
  }

  private async read(file: File): Promise<FinderResult> {
    const result = await readFrames(file);
    // kept for the next visit; a browser that cannot keep it still has it now
    this.store.set(KEY + fingerprint(file), result).catch((err: unknown) => console.warn(err));
    return result;
  }
}
