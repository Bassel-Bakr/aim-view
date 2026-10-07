/**
 * The desktop app's `RecordingSource`. In: the app's library over its API (the VODs folder the user
 * picks in the system's dialog). Out: the recordings list, the folder action and each video's
 * address on the app's protocol, for the recordings page and the run page.
 */

import { HttpClient } from '@angular/common/http';
import { computed, inject, Service, signal } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { FolderAction } from '../../platform/recording-source';
import { ServerRecordings } from '../http/server-recordings';
import { DESKTOP_API } from './desktop-api';

/**
 * The desktop app's recordings: the review server's services, answered by the app itself
 * (service/src/library/). The user chooses the VODs folder in the system's folder dialog; videos
 * stream from the app.
 */
@Service()
export class DesktopRecordings extends ServerRecordings {
  /** Sends the folder request (the base class keeps its own client private). */
  private readonly client = inject(HttpClient);
  /** True while the system's folder dialog is open, so the button shows it is busy. */
  private readonly choosing = signal(false);
  /** Where files the user adds end up, as the add dialog tells the user. */
  override readonly addedFilesGo = "They are copied into the app's own folder.";
  /** The list's error in the app's words; null while the list loads or has loaded. */
  override readonly problem = computed<string | null>(() =>
    this.list.error() ? 'The app could not list the recordings.' : null,
  );
  /** The top bar's VODs folder button: opens the system's folder dialog. */
  override readonly folder = computed<FolderAction>(() => ({
    label: 'VODs folder',
    detail: 'Choose the folder OBS records into (one folder per scenario)',
    busy: this.choosing(),
    run: () => this.chooseFolder(),
    files: null,
  }));

  /** The recording's video on the app's protocol, which streams it from disk. */
  protected override streamUrl(id: string): string {
    return `${DESKTOP_API}/video?id=${encodeURIComponent(id)}`;
  }

  /**
   * Asks the app to open the system's folder dialog (POST /api/folder), then reloads the list from
   * the folder chosen. The dialog's cancel leaves the folder as it was.
   */
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
