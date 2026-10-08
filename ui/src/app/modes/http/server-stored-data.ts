/**
 * The `StoredData` of every mode (in browser mode the service in the page answers it, in the
 * desktop app its own protocol). In: the review service's /api/storage. Out: what is kept and its
 * sizes, and a part removed, for the Storage panel.
 */

import { HttpClient, httpResource, HttpResourceRef } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { KeptData, StoredData } from '../../platform/stored-data';

/** The service's route for what is kept. */
const STORAGE = '/api/storage';

/** What the review service keeps (GET /api/storage) and removing a part (POST ?remove=). */
@Service()
export class ServerStoredData implements StoredData {
  /** Sends a removal. */
  private readonly http = inject(HttpClient);

  /** What is kept (GET /api/storage). */
  kept(): HttpResourceRef<KeptData | undefined> {
    return httpResource<KeptData>(() => STORAGE);
  }

  /** Removes the part (POST /api/storage?remove=id). */
  remove(id: string): Promise<KeptData> {
    return firstValueFrom(this.http.post<KeptData>(STORAGE, null, { params: { remove: id } }));
  }
}
