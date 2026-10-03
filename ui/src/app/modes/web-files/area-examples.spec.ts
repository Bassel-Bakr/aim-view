import { provideHttpClient } from '@angular/common/http';
import { provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { AreaExample, AreaKind } from '../../api';
import { answer, ApiRoutes } from '../../fake-api';
import { AreaExamples, exampleLine, exampleRec } from './area-examples';

const AIR = 'Air/Air - 1 - 2026.10.01-16.23.03.mp4';
const ADDED = 'uploads/b.mp4';

const lines = (examples: readonly AreaExample[]) =>
  examples.map((e) => `${exampleLine(e)}\n`).join('');

/** The review server's area finder files, as `bun run assets` ships them beside the app. */
const SHIPPED: ApiRoutes = {
  'data/area_examples.jsonl': lines([
    { rec: AIR, feat: [0.1, 0.2], kind: 'timer' },
    { rec: `kovobs:${AIR}`, feat: [0.3, 0.4], kind: 'clock' },
    { rec: ADDED, feat: [0.5, 0.6], kind: 'webcam' },
  ]),
  'data/area_kinds.json': JSON.stringify([
    { id: 'timer', name: 'Timer', about: "the run's time left" },
    { id: 'webcam', name: 'Webcam', about: 'a hand cam' },
  ]),
};

/** The store, its shipped files answered from routes (any other file is not there). */
async function examples(routes: ApiRoutes): Promise<AreaExamples> {
  TestBed.configureTestingModule({ providers: [provideHttpClient(), provideHttpClientTesting()] });
  const store = TestBed.inject(AreaExamples);
  await answer(routes);
  await store.ready;
  return store;
}

const named = (examples: readonly AreaExample[]) => examples.map((e) => [e.rec, e.kind]);
const names = (kinds: readonly AreaKind[]) => kinds.map((k) => `${k.id}: ${k.name}`);

describe('the area examples in the browser', () => {
  it("start from the review server's files shipped with the app", async () => {
    const store = await examples(SHIPPED);
    expect(store.count()).toEqual({ examples: 3, recordings: 2, kinds: 2 });
    expect([...store.labelled()].sort()).toEqual([AIR, ADDED]);
  });

  it("let a recording's areas saved here replace its shipped examples, its layout's too", async () => {
    const store = await examples(SHIPPED);
    await store.learnt(`folder:${AIR}`, [{ rec: 'x', feat: [0.9, 0.9], kind: 'webcam' }]);
    expect(named(store.examples())).toEqual([
      [ADDED, 'webcam'],
      [AIR, 'webcam'],
    ]);
  });

  it("let a file loaded here replace a recording's shipped examples", async () => {
    const store = await examples(SHIPPED);
    const file = lines([{ rec: ADDED, feat: [0.7, 0.8], kind: 'clock' }]);
    await store.load([new File([file], 'area_examples.jsonl')]);
    expect(named(store.examples())).toEqual([
      [AIR, 'timer'],
      [`kovobs:${AIR}`, 'clock'],
      [ADDED, 'clock'],
    ]);
  });

  it('let a type changed here win over the shipped one, and add the new ones after them', async () => {
    const store = await examples(SHIPPED);
    await store.setKinds([
      { id: 'timer', name: 'Round timer', about: '' },
      { id: 'kill_feed', name: 'Kill feed', about: '' },
    ]);
    expect(names(store.kinds())).toEqual([
      'timer: Round timer',
      'webcam: Webcam',
      'kill_feed: Kill feed',
    ]);
  });

  it('start empty without the files (a build for others), or with a page in their place', async () => {
    expect((await examples({})).count()).toEqual({ examples: 0, recordings: 0, kinds: 0 });
    TestBed.resetTestingModule();
    const page = '<!doctype html><html></html>';
    const store = await examples({
      'data/area_examples.jsonl': page,
      'data/area_kinds.json': page,
    });
    expect(store.count()).toEqual({ examples: 0, recordings: 0, kinds: 0 });
  });

  it('name a recording opened here as the review server names it', () => {
    expect(exampleRec(`folder:${AIR}`)).toBe(AIR);
    expect(exampleRec('local:3/b.mp4')).toBe(ADDED);
  });
});
