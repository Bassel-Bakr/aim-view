import { HttpRequest } from '@angular/common/http';
import { TestBed } from '@angular/core/testing';
import { ModelList } from '../api';
import { ApiRoutes } from '../fake-api';
import { MODE_CASES, setUp } from './contract-case';
import { ModelCatalog } from './model-catalog';

/** A review server with two models, the way python/server.py lists them. */
function fakeServer(): ApiRoutes {
  const list = (chosen: string): ModelList => ({
    chosen,
    device: 'cuda',
    speed: '',
    checked_on: '',
    checks: [],
    models: [
      { name: 'full_v3', label: 'full_v3', available: true, default: true },
      { name: 'small_v13', label: 'small_v13', available: true },
    ],
  });
  let chosen = 'full_v3';
  return {
    '/api/models': () => list(chosen),
    '/api/model': (req: HttpRequest<unknown>) => list((chosen = req.params.get('name') ?? '')),
  };
}

for (const mode of MODE_CASES) {
  describe(`ModelCatalog (${mode.name} mode)`, () => {
    afterEach(() => localStorage.clear());

    it('lists the models, the default among them and the one in use', async () => {
      const catalog = setUp(mode, ModelCatalog);
      const routes = fakeServer();
      await mode.finish(new Promise((r) => setTimeout(r)), routes);
      TestBed.tick();
      const list = await mode.finish(
        new Promise<ModelList | undefined>((r) => setTimeout(() => r(catalog.list.value()))),
        routes,
      );
      expect(list?.chosen).toBe('full_v3');
      expect(list?.models.find((m) => m.default)?.name).toBe('full_v3');
      expect(list?.models.map((m) => m.name)).toContain('small_v13');
    });

    it('keeps the pick', async () => {
      const catalog = setUp(mode, ModelCatalog);
      const list = await mode.finish(catalog.pick('small_v13'), fakeServer());
      expect(list.chosen).toBe('small_v13');
    });
  });
}
