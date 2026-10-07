/**
 * Server mode's `FaintCutoffs`, which the desktop app uses too and browser mode extends. In: the
 * review service's /api/faint routes. Out: each recording's cut-off and the cut-off queue, for the
 * run page's Cut-off and the top bar's Cut-off queue.
 */

import { HttpClient, httpResource, HttpResourceRef } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { FaintChoice, FaintSetting, Job } from '../../api';
import { CutoffLabelsStore, FaintCutoffs } from '../../platform/faint-cutoffs';

/**
 * The cut-off on the review service (service/src/faint.rs): faint.json in the recording's folder
 * (/api/faint), which a tracking run's review is measured again with; a submit writes its labels
 * in the data folder's cutoff folder, test_out/vod_model/hand/cutoff/ in Python's layout
 * (/api/faint_submit); the queue (/api/faint_queue) and its skips (/api/faint_skip,
 * faint_skipped.json).
 */
@Service()
export class ServerFaintCutoffs implements FaintCutoffs {
  /** Sends the save, submit, queue and skip requests. */
  private readonly http = inject(HttpClient);
  /** The service writes the labels where training reads them, so the page keeps none. */
  readonly labels: CutoffLabelsStore | null = null;

  /** The recording's cut-off as the service keeps it (GET /api/faint). */
  setting(id: () => string | undefined): HttpResourceRef<FaintSetting | undefined> {
    return httpResource<FaintSetting>(() => {
      const at = id();
      return at === undefined ? undefined : { url: '/api/faint', params: { id: at } };
    });
  }

  /**
   * Keeps the cut-off (POST /api/faint), then gives the recording's job: the service measures a
   * tracking run again when the cut changes, so it is that job, or the one before.
   */
  async save(id: string, choice: FaintChoice): Promise<Job> {
    await firstValueFrom(this.http.post<FaintSetting>('/api/faint', choice, { params: { id } }));
    return firstValueFrom(this.http.get<Job>('/api/job', { params: { id } }));
  }

  /** Submits the cut-off at the offset (POST /api/faint_submit); the service writes the labels. */
  submit(id: string, offset: number): Promise<FaintSetting> {
    const params = { id, offset: String(offset) };
    return firstValueFrom(this.http.post<FaintSetting>('/api/faint_submit', null, { params }));
  }

  /** The recording ids to set a cut-off in, as the service orders them (GET /api/faint_queue). */
  queue(): Promise<string[]> {
    return firstValueFrom(this.http.get<string[]>('/api/faint_queue'));
  }

  /** Leaves the recording out of the queue (POST /api/faint_skip, kept in faint_skipped.json). */
  async skip(id: string): Promise<void> {
    await firstValueFrom(this.http.post('/api/faint_skip', null, { params: { id } }));
  }
}
