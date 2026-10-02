import { TestBed } from '@angular/core/testing';
import { BrowserStore, StoreEntry } from './browser-store';
import { StatsCache } from './stats-cache';

describe('StatsCache', () => {
  it('keeps the stats files, then only those that are new or changed, and reads them back', async () => {
    const kept = new Map<string, unknown>();
    let stored = 0;
    TestBed.configureTestingModule({
      providers: [
        {
          provide: BrowserStore,
          useValue: {
            get: async (k: string) => kept.get(k),
            set: async (k: string, v: unknown) => void kept.set(k, v),
            setMany: async (entries: readonly StoreEntry[]) => {
              for (const [k, v] of entries) kept.set(k, v);
              stored += entries.length;
            },
          },
        },
      ],
    });
    const cache = TestBed.inject(StatsCache);
    const file = (name: string, text: string, modified = 1) =>
      new File([text], name, { lastModified: modified });
    expect(await cache.names()).toBeNull();
    await cache.keep([file('a.csv', 'a'), file('b.csv', 'b')], () => undefined);
    expect(stored).toBe(2);
    await cache.keep(
      [file('a.csv', 'a'), file('b.csv', 'bb', 2), file('c.csv', 'c')],
      () => undefined,
    );
    expect(stored).toBe(4);
    expect((await cache.names())?.sort()).toEqual(['a.csv', 'b.csv', 'c.csv']);
    expect(await (await cache.read('b.csv')).text()).toBe('bb');
  });
});
