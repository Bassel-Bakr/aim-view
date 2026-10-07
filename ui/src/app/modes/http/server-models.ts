/**
 * The `ModelCatalog` of every mode (in browser mode the service in the page answers it). In: the
 * review service's /api/models, /api/model, /api/device and /api/batch. Out: the models and the
 * review's settings for the model panel; each pick gives the list as it is after it.
 */

import { HttpClient, httpResource } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { Device, ModelList } from '../../api';
import { ModelCatalog } from '../../platform/model-catalog';

/**
 * The review server's models (/api/models), and what its reviews use, kept across restarts: the
 * model (/api/model), the device (/api/device: the ones the server can run) and the frames at
 * once, for each device (/api/batch).
 */
@Service()
export class ServerModels implements ModelCatalog {
  /** Sends the picks. */
  private readonly http = inject(HttpClient);
  /** The models, the one in use, the devices and the frames at once (GET /api/models). */
  readonly list = httpResource<ModelList>(() => '/api/models');

  /** Picks the model new reviews use (POST /api/model). */
  pick(name: string): Promise<ModelList> {
    return firstValueFrom(this.http.post<ModelList>('/api/model', null, { params: { name } }));
  }

  /** Picks the device new reviews run the detector on (POST /api/device). */
  useDevice(device: Device): Promise<ModelList> {
    return firstValueFrom(
      this.http.post<ModelList>('/api/device', null, { params: { name: device } }),
    );
  }

  /** Picks the frames the detector takes at once on the device in use (POST /api/batch). */
  useBatch(batch: number): Promise<ModelList> {
    // eslint-disable-next-line id-length -- the API's name for the frame count
    return firstValueFrom(this.http.post<ModelList>('/api/batch', null, { params: { n: batch } }));
  }
}
