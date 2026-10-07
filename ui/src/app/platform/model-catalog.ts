/**
 * The ModelCatalog contract: the detector models, the one picked, and where and how they run.
 * In: each mode's implementation (modes/mode.*.ts). Out: the Models service (services/models.ts)
 * and the model panel.
 */

import { ResourceRef } from '@angular/core';
import { Device, ModelList } from '../api';

/** The detector models and the one new reviews use. Each mode provides one (modes/mode.*.ts). */
export abstract class ModelCatalog {
  /** The models, the one picked, the device and the frames at once (/api/models). */
  abstract readonly list: ResourceRef<ModelList | undefined>;

  /** Picks the model new reviews use; resolves to the list as it is now. */
  abstract pick(name: string): Promise<ModelList>;

  /** Picks where new reviews run the detector (one of the list's devices); resolves to the list as it is now. */
  abstract useDevice(device: Device): Promise<ModelList>;

  /** Picks how many frames new reviews give the detector at once on the device in use (one of the list's batches). */
  abstract useBatch(batch: number): Promise<ModelList>;
}
