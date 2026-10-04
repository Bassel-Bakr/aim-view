import { HttpClient } from '@angular/common/http';
import { inject, Injectable } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { NotAimMark } from '../../api';
import { ExamplesStore, Labelling } from '../../platform/labelling';
import { RecordingSource } from '../../platform/recording-source';

/**
 * Labelling on the review server: its queue (/api/label_queue), skips (/api/label_skip, label_skipped.json) and other
 * games (/api/not_aim, not_aim_trainer.json). The server writes the area finder's examples itself when areas are saved.
 */
@Injectable({ providedIn: 'root' })
export class ServerLabelling implements Labelling {
  private readonly http = inject(HttpClient);
  private readonly source = inject(RecordingSource);
  readonly examples: ExamplesStore | null = null;

  queue(): Promise<string[]> {
    return firstValueFrom(this.http.get<string[]>('/api/label_queue'));
  }

  async skip(id: string): Promise<void> {
    await firstValueFrom(this.http.post('/api/label_skip', null, { params: { id } }));
  }

  async setNotAim(id: string, on: boolean): Promise<void> {
    const params = { id, on: on ? '1' : '0' };
    const mark = await firstValueFrom(this.http.post<NotAimMark>('/api/not_aim', null, { params }));
    this.source.patch(id, { not_aim: mark.not_aim });
  }
}
