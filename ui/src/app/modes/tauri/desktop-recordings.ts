import { HttpClient } from '@angular/common/http';
import { computed, inject, Injectable, signal } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { FolderAction, VideoState } from '../../platform/recording-source';
import { ServerRecordings } from '../http/server-recordings';
import { DESKTOP_API } from './desktop-api';

/**
 * The desktop app's recordings: the review server's services, answered by the app itself (desktop/src/library.rs).
 * The user chooses the VODs folder in the system's folder dialog; videos stream from the app.
 */
@Injectable({ providedIn: 'root' })
export class DesktopRecordings extends ServerRecordings {
  private readonly client = inject(HttpClient);
  private readonly choosing = signal(false);
  override readonly addedFilesGo = "They are copied into the app's own folder.";
  override readonly problem = computed<string | null>(() =>
    this.list.error() ? 'The app could not list the recordings.' : null,
  );
  override readonly folder = computed<FolderAction>(() => ({
    label: 'VODs folder',
    detail: 'Choose the folder OBS records into (one folder per scenario)',
    busy: this.choosing(),
    run: () => this.chooseFolder(),
    files: null,
  }));

  override video(id: string): VideoState {
    return {
      state: 'ready',
      url: `${DESKTOP_API}/video?id=${encodeURIComponent(id)}`,
      remuxed: false,
    };
  }

  /** The system's folder dialog; the list follows the folder chosen. */
  private async chooseFolder(): Promise<void> {
    this.choosing.set(true);
    try {
      await firstValueFrom(this.client.post('/api/folder', null));
      this.list.reload();
    } finally {
      this.choosing.set(false);
    }
  }
}
