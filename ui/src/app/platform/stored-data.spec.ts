import { HttpRequest } from '@angular/common/http';
import { TestBed } from '@angular/core/testing';
import { storageRow } from '../storage-panel/storage-panel';
import { MODE_CASES, setUp } from './contract-case';
import { KeptData, StoredData } from './stored-data';

/** What a review service keeps: two models' reviews, the user's marks and the videos added. */
const KEPT: KeptData = {
  total: 5_000_000,
  database: 3_000_000,
  parts: [
    {
      id: 'review:a',
      kind: 'reviews',
      bytes: 2_000_000,
      removable: true,
      model: 'a',
      recordings: 2,
      listed: false,
    },
    { id: 'marks', kind: 'marks', bytes: 1000, removable: false },
    { id: 'uploads', kind: 'uploads', bytes: 2_000_000, removable: true, files: 1 },
  ],
};

/** A service that answers what it keeps, and after removing a part, the rest. */
function routes(removed: string[]) {
  return {
    '/api/storage': (req: HttpRequest<unknown>) => {
      if (req.method !== 'POST') return KEPT;
      const id = req.params.get('remove') ?? '';
      removed.push(id);
      return { ...KEPT, parts: KEPT.parts.filter((part) => part.id !== id) };
    },
  };
}

for (const mode of MODE_CASES) {
  describe(`StoredData (${mode.name} mode)`, () => {
    it('reads what is kept and removes a part', async () => {
      const stored = setUp(mode, StoredData);
      const removed: string[] = [];
      const ref = TestBed.runInInjectionContext(() => stored.kept());
      const read = mode.finish(
        (async () => {
          for (;;) {
            TestBed.tick();
            await new Promise((resolve) => setTimeout(resolve));
            if (ref.hasValue() && !ref.isLoading()) return ref.value();
          }
        })(),
        routes(removed),
      );
      expect((await read)?.parts.length).toBe(3);
      const after = await mode.finish(stored.remove('review:a'), routes(removed));
      expect(removed).toEqual(['review:a']);
      expect(after.parts.map((part) => part.id)).toEqual(['marks', 'uploads']);
    });
  });
}

describe('the Storage panel rows', () => {
  it('names each part and says what removing it does', () => {
    const [reviews, marks, uploads] = KEPT.parts.map(storageRow);
    expect(reviews.name).toBe('Reviews by a');
    expect(reviews.detail).toBe('2 recordings; this model is no longer offered');
    expect(reviews.removes).not.toBe('');
    expect(marks.removable).toBe(false);
    expect(uploads.detail).toBe('1 file');
  });
});
