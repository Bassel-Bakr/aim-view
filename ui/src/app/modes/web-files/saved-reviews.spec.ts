import { TestBed } from '@angular/core/testing';
import { Tracks } from '../../api';
import { BrowserStore, StoreEntry } from './browser-store';
import { fingerprint, SavedReview, SavedReviews } from './saved-reviews';

/** A store in memory, as IndexedDB keeps values across visits. */
class MemoryStore {
  readonly kept = new Map<string, unknown>();
  async get<T>(key: string): Promise<T | undefined> {
    return structuredClone(this.kept.get(key)) as T | undefined;
  }
  async setMany(entries: readonly StoreEntry[]): Promise<void> {
    for (const [k, v] of entries) this.kept.set(k, structuredClone(v));
  }
}

const review = (model: string): SavedReview => ({
  tracks: { fps: 60, frames: [] } as Tracks,
  readings: { camera: [], countdown: [] },
  model,
});

/** A new page on the same store: the saved reviews as the next visit finds them. */
function visit(store: MemoryStore): SavedReviews {
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({ providers: [{ provide: BrowserStore, useValue: store }] });
  return TestBed.inject(SavedReviews);
}

describe('SavedReviews', () => {
  const file = new File(['video'], 'Controlsphere - 13278 - 2026.08.11-13.55.27.mp4', {
    lastModified: 1_700_000_000_000,
  });

  it('keeps a review for the next visit, one per model, the chosen one shown first', async () => {
    const store = new MemoryStore();
    const now = visit(store);
    await now.save(file, review('small_v13'));
    await new Promise((r) => setTimeout(r, 5));
    await now.save(file, review('full_v3'));

    const next = visit(store);
    await next.load(file, 'full_v3');
    expect(next.has(file)).toBe(true);
    expect(next.shownModel(file, 'small_v13')).toBe('small_v13');
    // a model with no review of it: the latest saved
    expect(next.shownModel(file, 'small_v11')).toBe('full_v3');
    expect((await next.load(file, 'small_v13'))?.model).toBe('small_v13');
  });

  it('does not match a changed file to an old review', async () => {
    const store = new MemoryStore();
    await visit(store).save(file, review('full_v3'));
    const next = visit(store);
    await next.load(file, 'full_v3');
    const changed = new File(['video, cut'], file.name, { lastModified: file.lastModified });
    expect(fingerprint(changed)).not.toBe(fingerprint(file));
    expect(next.has(changed)).toBe(false);
    expect(next.shownModel(changed, 'full_v3')).toBeNull();
    expect(await next.load(changed, 'full_v3')).toBeNull();
  });
});
