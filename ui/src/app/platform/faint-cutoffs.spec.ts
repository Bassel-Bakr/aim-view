import { TestBed } from '@angular/core/testing';
import { FaintSetting } from '../api';
import { MODE_CASES, setUp } from './contract-case';
import { FaintCutoffs } from './faint-cutoffs';
import { ReviewEngine } from './review-engine';

const ID = 'Air/Air - 1 - 2026.10.01-16.23.03.mp4';
const SAVED: FaintSetting = { on: true, offset: 0.35 };

for (const mode of MODE_CASES) {
  describe(`FaintCutoffs (${mode.name} mode)`, () => {
    it('keeps a cut-off for a recording it can review, and refuses one it cannot', async () => {
      const cutoffs = setUp(mode, FaintCutoffs);
      const can = TestBed.inject(ReviewEngine).unavailable(ID) === null;
      const routes = { '/api/faint': SAVED, '/api/job': { stage: 'measuring' } };
      const job = await mode.finish(
        cutoffs.save(ID, { on: true, offset: 0.35 }).catch((e: unknown) => ({
          stage: 'error',
          error: String(e),
        })),
        routes,
      );
      if (can) expect(job.stage).not.toBe('error');
      else expect(job.stage).toBe('error');
    });

    it('refuses to submit a recording without a review', async () => {
      const cutoffs = setUp(mode, FaintCutoffs);
      const done = await mode.finish(
        cutoffs.submit(ID, 0.3).then(
          () => 'submitted',
          () => 'refused',
        ),
        {},
      );
      expect(done).toBe('refused');
    });

    it('gives the queue of recordings to set a cut-off in', async () => {
      const cutoffs = setUp(mode, FaintCutoffs);
      const queue = await mode.finish(cutoffs.queue(), { '/api/faint_queue': [ID] });
      expect(Array.isArray(queue)).toBe(true);
      if (mode.name === 'server') expect(queue).toEqual([ID]);
    });

    it('keeps the labels where training reads them, or here to download', () => {
      const cutoffs = setUp(mode, FaintCutoffs);
      if (mode.name === 'browser')
        expect(cutoffs.labels?.count()).toEqual({ crops: 0, recordings: 0 });
      else expect(cutoffs.labels).toBeNull();
    });
  });
}
