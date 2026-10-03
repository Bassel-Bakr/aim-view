import { HttpClient, httpResource } from '@angular/common/http';
import { inject, Injectable } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { Device, ModelList } from '../../api';
import { ModelCatalog } from '../../platform/model-catalog';

/** The review server's models (/api/models), and the one it reviews with (/api/model, kept across restarts). */
@Injectable({ providedIn: 'root' })
export class ServerModels implements ModelCatalog {
  private readonly http = inject(HttpClient);
  readonly list = httpResource<ModelList>(() => '/api/models');

  pick(name: string): Promise<ModelList> {
    return firstValueFrom(this.http.post<ModelList>('/api/model', null, { params: { name } }));
  }

  /** The review server runs on the device it has: the page does not choose. */
  async useDevice(device: Device): Promise<ModelList> {
    throw new Error(`The review server picks its own device, not ${device}`);
  }

  /** The review server runs its detector as it does: the page does not choose. */
  async useBatch(batch: number): Promise<ModelList> {
    throw new Error(`The review server runs its own detector, not ${batch} frames at once`);
  }
}
