/**
 * Server mode's `Labelling`, which the desktop app uses too and browser mode extends. In: the
 * review service's /api/label_queue, /api/label_skip and /api/not_aim. Out: the area queue and the
 * other-game mark, for the labelling tools; a mark also updates the recording's row.
 */

import { HttpClient } from '@angular/common/http';
import { inject, Service } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { NotAimMark } from '../../api';
import { ExamplesStore, Labelling } from '../../platform/labelling';
import { RecordingSource } from '../../platform/recording-source';

/**
 * Labelling on the review server (service/src/labels.rs): its queue (/api/label_queue), skips
 * (/api/label_skip, label_skipped.json) and other games (/api/not_aim, not_aim_trainer.json). The
 * server writes the area finder's examples itself when areas are saved.
 */
@Service()
export class ServerLabelling implements Labelling {
  /** Sends the queue, skip and mark requests. */
  private readonly http = inject(HttpClient);
  /** The recordings list, whose row a mark changes. */
  private readonly source = inject(RecordingSource);
  /** The service keeps the area finder's examples itself, so the page offers none to download. */
  readonly examples: ExamplesStore | null = null;

  /** The recording ids to label areas in, as the service orders them (GET /api/label_queue). */
  queue(): Promise<string[]> {
    return firstValueFrom(this.http.get<string[]>('/api/label_queue'));
  }

  /** Leaves the recording out of the queue (POST /api/label_skip, kept in label_skipped.json). */
  async skip(id: string): Promise<void> {
    await firstValueFrom(this.http.post('/api/label_skip', null, { params: { id } }));
  }

  /** Marks the recording as another game or not (POST /api/not_aim), then patches its row. */
  async setNotAim(id: string, on: boolean): Promise<void> {
    const params = { id, on: on ? '1' : '0' };
    const mark = await firstValueFrom(this.http.post<NotAimMark>('/api/not_aim', null, { params }));
    this.source.patch(id, { not_aim: mark.not_aim });
  }
}
