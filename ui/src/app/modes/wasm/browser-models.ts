import { Injectable, resource } from '@angular/core';
import { Check, Device, Model, ModelList } from '../../api';
import { ModelCatalog } from '../../platform/model-catalog';
import MODELS_FILE from '../../../../../python/model/models.json';

/** A model's label in models.json: there only when it differs from the name. */
export interface ModelLabel {
  label?: string;
}

/** A model as models.json describes it: no name (its key), and maybe a label. */
export type ModelEntry = Omit<Model, 'name' | 'label' | 'available'> & ModelLabel;

/** python/model/models.json: what the model picker shows about each model. */
export interface ModelsFile {
  speed: string;
  checked_on: string;
  checks: Check[];
  models: Record<string, ModelEntry>;
}

// TypeScript reads the JSON's pairs ([kills matched, flicks measured]) as plain arrays: the file's own type says them
const MODELS = MODELS_FILE as unknown as ModelsFile;
const CHOSEN_KEY = 'aimview-model';
const DEVICE_KEY = 'aimview-device';
/** Whether this browser can run the detector on the GPU (WebGPU). */
const HAS_GPU = typeof navigator !== 'undefined' && 'gpu' in navigator;
/** Where the browser can run the detector: the GPU first, where there is one. */
const DEVICES: Device[] = HAS_GPU ? ['webgpu', 'wasm'] : ['wasm'];
/**
 * How many frames the detector can take at once, and how many it takes until the user picks: on the GPU, 4 (av1 at
 * 2560x1440: 100 frames a second one at a time, 148 four at a time, 153 eight at a time, the same boxes); on the CPU,
 * 1 (37 one at a time, 28 four at a time). Machines differ, so the user can pick.
 */
const BATCHES = [1, 2, 4, 8];
const DEFAULT_BATCH: Record<Device, number> = { webgpu: 4, wasm: 1, cuda: 1, cpu: 1, directml: 4 };
const BATCH_KEY = 'aimview-batch-';
/** The model the browser reviews with until the user picks one: the best on every check (infer.BEST). */
const DEFAULT_MODEL = 'full_v3';
/** The hand-written detector is Python code, not a model file: it runs here once the review core has it. */
const HAND = 'hand';
const NOT_PORTED = 'Not in the browser yet (Python code, not a model file)';

/**
 * The models as the browser runs them: every one with an ONNX export, on onnxruntime-web, on the GPU (WebGPU) or the
 * CPU (WebAssembly) as the user picks (MODEL_STATUS.md, "Browser"). The hand-written detector waits for the review
 * core.
 */
export function browserModels(
  file: ModelsFile,
  chosen: string,
  device: Device,
  batch = DEFAULT_BATCH[device],
): ModelList {
  return {
    chosen,
    device,
    devices: DEVICES,
    batch,
    batches: BATCHES,
    speed: file.speed,
    checked_on: file.checked_on,
    checks: file.checks,
    models: Object.entries(file.models).map(([name, m]) => ({
      ...m,
      name,
      label: m.label ?? name,
      default: name === DEFAULT_MODEL,
      available: name !== HAND,
    })),
    unavailable: NOT_PORTED,
  };
}

/** The models the browser can run (models.json, built into this mode's code), and the user's pick, kept here. */
@Injectable({ providedIn: 'root' })
export class BrowserModels implements ModelCatalog {
  readonly list = resource({
    loader: async () => this.now(),
  });

  async pick(name: string): Promise<ModelList> {
    localStorage.setItem(CHOSEN_KEY, name);
    return this.now();
  }

  async useDevice(device: Device): Promise<ModelList> {
    if (!DEVICES.includes(device))
      throw new Error(`This browser cannot run the detector on ${device}`);
    localStorage.setItem(DEVICE_KEY, device);
    return this.now();
  }

  /** Kept for the device in use: the GPU and the CPU each keep their own. */
  async useBatch(batch: number): Promise<ModelList> {
    if (!BATCHES.includes(batch))
      throw new Error(`The detector does not take ${batch} frames at once`);
    localStorage.setItem(BATCH_KEY + this.device(), String(batch));
    return this.now();
  }

  /** The list with the user's picks as they are now. */
  private now(): ModelList {
    const device = this.device();
    return browserModels(MODELS, this.chosen(), device, this.batch(device));
  }

  /** How many frames at once the user picked for the device; else its default. */
  private batch(device: Device): number {
    const kept = Number(localStorage.getItem(BATCH_KEY + device));
    return BATCHES.includes(kept) ? kept : DEFAULT_BATCH[device];
  }

  private chosen(): string {
    return localStorage.getItem(CHOSEN_KEY) ?? DEFAULT_MODEL;
  }

  /** The device the user picked, while this browser has it; else the first it has (the GPU, where there is one). */
  private device(): Device {
    const kept = localStorage.getItem(DEVICE_KEY) as Device | null;
    return kept && DEVICES.includes(kept) ? kept : DEVICES[0];
  }
}
