import { MODE_CASES, setUp } from './contract-case';
import { ReviewEngine } from './review-engine';

const ID = 'Air/Air - 1 - 2026.10.01-16.23.03.mp4';

for (const mode of MODE_CASES) {
  describe(`ReviewEngine (${mode.name} mode)`, () => {
    it('says why it cannot review a recording, and a review it starts then fails with that', async () => {
      const engine = setUp(mode, ReviewEngine);
      const why = engine.unavailable(ID);
      const routes = { '/api/analyse': { stage: 'starting' } };
      const job = await mode.finish(engine.start(ID, false), routes);
      if (why === null) expect(job.stage).not.toBe('error');
      else expect(job).toEqual({ stage: 'error', error: why });
    });

    it('answers how a recording’s job stands', async () => {
      const engine = setUp(mode, ReviewEngine);
      const job = await mode.finish(engine.job(ID), { '/api/job': { stage: 'none' } });
      expect(job.stage).toBe('none');
    });
  });
}
