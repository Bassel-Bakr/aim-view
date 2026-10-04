import { HttpRequest } from '@angular/common/http';
import { TestBed } from '@angular/core/testing';
import { Job } from '../api';
import { answer, ApiRoutes, recording, serverMode } from '../fake-api';
import { Library } from './library';
import { Review } from './review';

const ID = 'x/run.mp4';

/** Answers the app's requests until the condition holds (the job is polled every 0.5 s), or fails after 4 s. */
async function serveUntil(routes: ApiRoutes, condition: () => boolean): Promise<void> {
  for (let i = 0; i < 80 && !condition(); i++) {
    await answer(routes);
    if (!condition()) await new Promise((resolve) => setTimeout(resolve, 50));
  }
  expect(condition()).toBe(true);
}

function open(): Review {
  TestBed.configureTestingModule({ providers: serverMode() });
  TestBed.inject(Library).selectedId.set(ID);
  return TestBed.inject(Review);
}

describe('Review', () => {
  afterEach(() => history.replaceState(null, '', '/'));

  it('runs a review, follows its job, then reloads the report and marks the recording reviewed', async () => {
    const stages: Job[] = [
      { stage: 'tracking', done: 10, total: 100 },
      { stage: 'measuring' },
      { stage: 'done', seconds: 3 },
    ];
    let started: string | null = null;
    let reports = 0;
    const routes: ApiRoutes = {
      '/api/vods': [recording({ id: ID, analysed: false })],
      '/api/job': () => (started ? (stages.shift() ?? stages.at(-1)) : { stage: 'none' }),
      '/api/analyse': (req: HttpRequest<unknown>) => {
        started = req.params.get('again') ?? 'no';
        return { stage: 'starting' };
      },
      '/api/report': () => (++reports > 1 ? { mode: 'click', review_model: 'full_v3' } : null),
    };
    const review = open();
    const library = TestBed.inject(Library);
    await serveUntil(routes, () => library.all().length > 0 && review.report.hasValue());

    const analysing = review.analyse(false);
    await serveUntil(routes, () => started !== null);
    await analysing;
    expect(started).toBe('no');
    await serveUntil(routes, () => review.report.value()?.review_model === 'full_v3');
    expect(review.job().stage).toBe('done');
    expect(library.selected()?.analysed).toBe(true);
  });

  it('shows why a review could not start', async () => {
    const routes: ApiRoutes = {
      '/api/vods': [recording({ id: ID })],
      '/api/job': { stage: 'none' },
      '/api/report': null,
    };
    const review = open();
    await serveUntil(routes, () => review.report.hasValue());
    const analysing = review.analyse(true);
    await answer({ ...routes, '/api/analyse': () => ({ stage: 'error', error: 'no video' }) });
    await analysing;
    expect(review.job()).toEqual({ stage: 'error', error: 'no video' });
  });
});
