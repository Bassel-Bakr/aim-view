import { Injectable, resource } from '@angular/core';
import { Check, Model, ModelList } from '../../api';
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
/** The model the browser reviews with until the user picks one: the best on every check (infer.BEST). */
const DEFAULT_MODEL = 'full_v3';
/** The hand-written detector is Python code, not a model file: it runs here once the review core has it. */
const HAND = 'hand';
const NOT_PORTED = 'Not in the browser yet (Python code, not a model file)';

/**
 * The models as the browser runs them: every one with an ONNX export, on onnxruntime-web's WebAssembly backend
 * (MODEL_STATUS.md, "Browser"). The hand-written detector waits for the review core.
 */
export function browserModels(file: ModelsFile, chosen: string): ModelList {
  return {
    chosen,
    device: 'wasm',
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
    loader: async () => browserModels(MODELS, localStorage.getItem(CHOSEN_KEY) ?? DEFAULT_MODEL),
  });

  async pick(name: string): Promise<ModelList> {
    localStorage.setItem(CHOSEN_KEY, name);
    return browserModels(MODELS, name);
  }
}
