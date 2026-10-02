import { ResourceRef } from '@angular/core';
import { Device, ModelList } from '../api';

/** The detector models and the one new reviews use. Each mode provides one (modes/mode.*.ts). */
export abstract class ModelCatalog {
  abstract readonly list: ResourceRef<ModelList | undefined>;

  /** Picks the model new reviews use; resolves to the list as it is now. */
  abstract pick(name: string): Promise<ModelList>;

  /** Picks where new reviews run the detector (one of the list's devices); resolves to the list as it is now. */
  abstract useDevice(device: Device): Promise<ModelList>;
}
