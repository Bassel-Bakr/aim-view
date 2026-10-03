import { HttpRequest } from '@angular/common/http';
import { TestBed } from '@angular/core/testing';
import { Device, ModelList } from '../api';
import { ApiRoutes, Refused } from '../fake-api';
import { MODE_CASES, setUp } from './contract-case';
import { ModelCatalog } from './model-catalog';

/**
 * A review server with two models, a GPU and the CPU, the way the service lists them (service/src/library/settings.rs),
 * refusing a device it cannot run or frames at once it does not offer, as the service does.
 */
function fakeServer(): ApiRoutes {
  let device: Device = 'directml';
  const batches = new Map<Device, number>();
  const list = (chosen: string): ModelList => ({
    chosen,
    device,
    devices: ['directml', 'cpu'],
    batch: batches.get(device) ?? 4,
    batches: [1, 2, 4, 8],
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
    '/api/device': (req: HttpRequest<unknown>) => {
      const name = req.params.get('name') as Device;
      if (!['directml', 'cpu'].includes(name))
        return new Refused(`the detector cannot run on ${name} here`);
      device = name;
      return list(chosen);
    },
    '/api/batch': (req: HttpRequest<unknown>) => {
      const n = Number(req.params.get('n'));
      if (![1, 2, 4, 8].includes(n)) return new Refused(`${n} frames at once is not a choice`);
      batches.set(device, n);
      return list(chosen);
    },
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

    it('lets the user choose the device where it can, and keeps the choice', async () => {
      const catalog = setUp(mode, ModelCatalog);
      if (mode.name === 'server') {
        const routes = fakeServer();
        const list = await mode.finish(catalog.useDevice('cpu'), routes);
        expect(list.device).toBe('cpu');
        expect(list.devices).toEqual(['directml', 'cpu']);
        await expect(mode.finish(catalog.useDevice('wasm'), routes)).rejects.toThrow();
        return;
      }
      const list = await catalog.useDevice('wasm');
      expect(list.device).toBe('wasm');
      expect(list.devices).toContain('wasm');
      await expect(catalog.useDevice('cuda')).rejects.toThrow();
    });

    it('lets the user choose the frames at once where it can, kept for each device', async () => {
      const catalog = setUp(mode, ModelCatalog);
      if (mode.name === 'server') {
        const routes = fakeServer();
        expect((await mode.finish(catalog.useBatch(8), routes)).batch).toBe(8);
        // the CPU's pick is the CPU's: the GPU keeps its 8
        expect((await mode.finish(catalog.useDevice('cpu'), routes)).batch).toBe(4);
        expect((await mode.finish(catalog.useBatch(1), routes)).batch).toBe(1);
        expect((await mode.finish(catalog.useDevice('directml'), routes)).batch).toBe(8);
        await expect(mode.finish(catalog.useBatch(3), routes)).rejects.toThrow();
        return;
      }
      await catalog.useDevice('wasm');
      expect((await catalog.useBatch(2)).batch).toBe(2);
      // the CPU's pick stays the CPU's
      expect((await catalog.useDevice('wasm')).batch).toBe(2);
      await expect(catalog.useBatch(3)).rejects.toThrow();
    });
  });
}
