import { computed, inject, Injectable } from '@angular/core';
import { Device, ModelList } from '../api';
import { ModelCatalog } from '../platform/model-catalog';

/** Where the detector runs, as the top bar says it. */
export const DEVICE_LABELS: Record<Device, string> = {
  cuda: 'GPU',
  cpu: 'CPU',
  wasm: 'browser CPU',
  webgpu: 'browser GPU',
};

/** The detector models, and the one new reviews use (the top bar and the run page show it). */
@Injectable({ providedIn: 'root' })
export class Models {
  private readonly catalog = inject(ModelCatalog);
  readonly list = this.catalog.list;
  /** The models, or null until they load. */
  readonly current = computed<ModelList | null>(() =>
    this.list.hasValue() ? (this.list.value() ?? null) : null,
  );
  /** The model new reviews use, or null until the list loads. */
  readonly chosen = computed<string | null>(() => this.current()?.chosen ?? null);

  /** Picks the model new reviews use. */
  async pick(name: string): Promise<void> {
    this.list.set(await this.catalog.pick(name));
  }

  /** Picks where new reviews run the detector. */
  async useDevice(device: Device): Promise<void> {
    this.list.set(await this.catalog.useDevice(device));
  }
}

/** A model's name as people read it: "hand" is the hand-written detector. */
export function modelName(name: string): string {
  return name === 'hand' ? 'hand-written' : name;
}
