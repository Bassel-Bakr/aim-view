import { HttpRequest } from '@angular/common/http';
import { TestBed } from '@angular/core/testing';
import { Device, ModelList } from '../api';
import { ApiRoutes, Refused } from '../fake-api';
import { MODE_CASES, setUp } from './contract-case';
import { ModelCatalog } from './model-catalog';

/** The devices a mode's service runs the detector on: the GPU first, then the CPU. */
type DevicePair = [gpu: Device, cpu: Device];

/** The review server's devices (DirectML and the CPU), and the browser's (WebGPU and WebAssembly). */
const DEVICES: Record<string, DevicePair> = {
  server: ['directml', 'cpu'],
  browser: ['webgpu', 'wasm'],
};

/**
 * A review server with two models, a GPU and the CPU, the way the service lists them (service/src/library/settings.rs),
 * refusing a device it cannot run or frames at once it does not offer, as the service does.
 */
function fakeModels([gpu, cpu]: DevicePair): ApiRoutes {
  let device: Device = gpu;
  const batches = new Map<Device, number>();
  const list = (chosen: string): ModelList => ({
    chosen,
    device,
    devices: [gpu, cpu],
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
      if (![gpu, cpu].includes(name)) return new Refused(`the detector cannot run on ${name} here`);
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
    const [gpu, cpu] = DEVICES[mode.name];
    const fakeServer = () => fakeModels([gpu, cpu]);

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

    it('lets the user choose the device, and keeps the choice', async () => {
      const catalog = setUp(mode, ModelCatalog);
      const routes = fakeServer();
      const list = await mode.finish(catalog.useDevice(cpu), routes);
      expect(list.device).toBe(cpu);
      expect(list.devices).toEqual([gpu, cpu]);
      await expect(mode.finish(catalog.useDevice('cuda'), routes)).rejects.toThrow();
    });

    it('lets the user choose the frames at once, kept for each device', async () => {
      const catalog = setUp(mode, ModelCatalog);
      const routes = fakeServer();
      expect((await mode.finish(catalog.useBatch(8), routes)).batch).toBe(8);
      // the CPU's pick is the CPU's: the GPU keeps its 8
      expect((await mode.finish(catalog.useDevice(cpu), routes)).batch).toBe(4);
      expect((await mode.finish(catalog.useBatch(1), routes)).batch).toBe(1);
      expect((await mode.finish(catalog.useDevice(gpu), routes)).batch).toBe(8);
      await expect(mode.finish(catalog.useBatch(3), routes)).rejects.toThrow();
    });
  });
}
