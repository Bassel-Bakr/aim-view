import { HttpClient, httpResource, HttpResourceRef } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { FaintChoice, FaintSetting, Job } from '../../api';
import { CutoffLabelsStore, FaintCutoffs } from '../../platform/faint-cutoffs';

/**
 * The cut-off on the review server (python/server.py): faint.json in the recording's folder (/api/faint), which a
 * tracking run's review is measured again with; a submit writes its labels in test_out/vod_model/hand/cutoff/
 * (/api/faint_submit); the queue (/api/faint_queue) and its skips (/api/faint_skip, faint_skipped.json).
 */
@Service()
export class ServerFaintCutoffs implements FaintCutoffs {
  private readonly http = inject(HttpClient);
  readonly labels: CutoffLabelsStore | null = null;

  setting(id: () => string | undefined): HttpResourceRef<FaintSetting | undefined> {
    return httpResource<FaintSetting>(() => {
      const at = id();
      return at === undefined ? undefined : { url: '/api/faint', params: { id: at } };
    });
  }

  /** The server measures a tracking run again when the cut changes: the job it started, or the one before. */
  async save(id: string, choice: FaintChoice): Promise<Job> {
    await firstValueFrom(this.http.post<FaintSetting>('/api/faint', choice, { params: { id } }));
    return firstValueFrom(this.http.get<Job>('/api/job', { params: { id } }));
  }

  submit(id: string, offset: number): Promise<FaintSetting> {
    const params = { id, offset: String(offset) };
    return firstValueFrom(this.http.post<FaintSetting>('/api/faint_submit', null, { params }));
  }

  queue(): Promise<string[]> {
    return firstValueFrom(this.http.get<string[]>('/api/faint_queue'));
  }

  async skip(id: string): Promise<void> {
    await firstValueFrom(this.http.post('/api/faint_skip', null, { params: { id } }));
  }
}
