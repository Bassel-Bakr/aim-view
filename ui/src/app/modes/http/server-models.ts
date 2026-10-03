import { HttpClient, httpResource } from '@angular/common/http';
import { inject, Injectable } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { Device, ModelList } from '../../api';
import { ModelCatalog } from '../../platform/model-catalog';

/**
 * The review server's models (/api/models), and what its reviews use, kept across restarts: the model (/api/model), the
 * device (/api/device: the ones the server can run) and the frames at once, for each device (/api/batch).
 */
@Injectable({ providedIn: 'root' })
export class ServerModels implements ModelCatalog {
  private readonly http = inject(HttpClient);
  readonly list = httpResource<ModelList>(() => '/api/models');

  pick(name: string): Promise<ModelList> {
    return firstValueFrom(this.http.post<ModelList>('/api/model', null, { params: { name } }));
  }

  useDevice(device: Device): Promise<ModelList> {
    return firstValueFrom(
      this.http.post<ModelList>('/api/device', null, { params: { name: device } }),
    );
  }

  useBatch(batch: number): Promise<ModelList> {
    return firstValueFrom(this.http.post<ModelList>('/api/batch', null, { params: { n: batch } }));
  }
}
