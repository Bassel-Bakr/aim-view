import { TestBed } from '@angular/core/testing';
import { Report } from '../api';
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

    it('keeps a run window for a recording it can review, and refuses one it cannot', async () => {
      const engine = setUp(mode, ReviewEngine);
      const marks = { start: 2, end: 30, length: null };
      const routes = { '/api/run': { stage: 'none' } };
      const job = await mode.finish(
        engine
          .setMarks(ID, marks)
          .catch((error: unknown) => ({ stage: 'error', error: String(error) })),
        routes,
      );
      if (engine.unavailable(ID) === null) expect(job.stage).not.toBe('error');
      else expect(job.stage).toBe('error');
    });

    it('keeps the report on show when it comes again unchanged, and takes a changed one', async () => {
      const engine = setUp(mode, ReviewEngine);
      let shown = { mode: 'click', fps: 60, kills: 1 };
      // a fresh copy each time, as the network gives it
      const routes = { '/api/report': () => ({ ...shown }) };
      const report = TestBed.runInInjectionContext(() => engine.report(() => ID));
      const loaded = () =>
        mode.finish(
          (async () => {
            for (;;) {
              TestBed.tick();
              await new Promise((resolve) => setTimeout(resolve));
              if (report.hasValue() && !report.isLoading()) return report.value();
            }
          })(),
          routes,
        );
      const first = (await loaded()) as Report;
      report.reload();
      expect(await loaded()).toBe(first);
      shown = { ...shown, kills: 2 };
      report.reload();
      expect(await loaded()).toEqual({ mode: 'click', fps: 60, kills: 2 });
    });

    it('cancels a recording’s review', async () => {
      const engine = setUp(mode, ReviewEngine);
      const job = await mode.finish(engine.cancel(ID), { '/api/cancel': { stage: 'cancelled' } });
      expect(job.stage).toBe('cancelled');
    });

    it('answers how a recording’s job stands', async () => {
      const engine = setUp(mode, ReviewEngine);
      const job = await mode.finish(engine.job(ID), { '/api/job': { stage: 'none' } });
      expect(job.stage).toBe('none');
    });
  });
}
