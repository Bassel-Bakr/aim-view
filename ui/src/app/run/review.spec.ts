import { TestBed } from '@angular/core/testing';
import { Job } from '../api';
import { fakeFetch, recording } from '../fake-api';
import { Library } from '../services/library';
import { Review } from './review';

const ID = 'x/run.mp4';

/** Waits until the condition holds, checking every 10 ms, or fails after 4 s (the job is polled every 0.5 s). */
async function until(condition: () => boolean): Promise<void> {
  for (let i = 0; i < 400 && !condition(); i++) await new Promise((r) => setTimeout(r, 10));
  expect(condition()).toBe(true);
}

describe('Review', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    history.replaceState(null, '', '/');
  });

  it('runs a review, follows its job, then reloads the report and marks the recording reviewed', async () => {
    const stages: Job[] = [
      { stage: 'tracking', done: 10, total: 100 },
      { stage: 'measuring' },
      { stage: 'done', seconds: 3 },
    ];
    let reports = 0;
    let started = '';
    vi.stubGlobal(
      'fetch',
      fakeFetch({
        '/api/vods': [recording({ id: ID, analysed: false })],
        '/api/job': () =>
          started ? (stages.shift() ?? { stage: 'done', seconds: 3 }) : { stage: 'none' },
        '/api/analyse': (url: URL) => {
          started = url.searchParams.get('again') ?? 'no';
          return { stage: 'starting' };
        },
        '/api/report': () => (++reports > 1 ? { mode: 'click', review_model: 'full_v3' } : null),
      }),
    );
    const library = TestBed.inject(Library);
    library.selectedId.set(ID);
    const review = TestBed.inject(Review);
    await until(() => library.recordings.hasValue() && review.report.hasValue());

    await review.analyse(false);
    expect(started).toBe('no');
    await until(() => review.job().stage === 'done');
    await until(() => review.report.value()?.review_model === 'full_v3');
    expect(library.selected()?.analysed).toBe(true);
  });

  it('shows a job that failed', async () => {
    vi.stubGlobal(
      'fetch',
      fakeFetch({
        '/api/vods': [recording({ id: ID })],
        '/api/job': { stage: 'none' },
        '/api/analyse': { stage: 'error', error: 'no video' },
        '/api/report': null,
      }),
    );
    TestBed.inject(Library).selectedId.set(ID);
    const review = TestBed.inject(Review);
    await review.analyse(true);
    await until(() => review.job().stage === 'error');
    expect(review.job().error).toBe('no video');
  });
});
