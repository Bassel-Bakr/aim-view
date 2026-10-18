/**
 * The desktop app's `RecordingSource`. In: the app's library over its API (the VODs folder the user
 * picks in the system's dialog) and the files the user adds, sent in parts. Out: the recordings list,
 * the folder action and each video's address on the app's protocol, for the recordings page and the
 * run page.
 */

import { HttpClient } from '@angular/common/http';
import { computed, inject, Service, signal } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { Uploaded } from '../../api';
import { FolderAction } from '../../platform/recording-source';
import { ServerRecordings, UploadParams } from '../http/server-recordings';
import { DESKTOP_API } from './desktop-api';

/**
 * The bytes of one upload request. The app's window hands each request to the app whole (on Windows
 * at about 25 times its size in memory: 1.2 GB in the app and 2.1 GB in WebView2 for 132 MB), and
 * each part is read into memory to be sent (desktop-api.ts), so a video goes in parts this size.
 */
const PART_BYTES = 8 * 1024 * 1024;

/**
 * The page's window as the Linux app leaves it: the address of the app's video server
 * (desktop/src/video_server.rs), since WebKitGTK plays no video from the app's own protocol.
 */
interface DesktopWindow extends Window {
  /** "http://127.0.0.1:<port>/<token>", to which /video?id=... is added; unset on Windows. */
  aimviewVideoBase?: string;
}

/** The service's answer to a part before the last: the upload's name for the next parts. */
interface UploadedPart {
  /** The name the next parts send as upload=. */
  upload: string;
  /** The bytes received so far. */
  received: number;
}

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

  /**
   * The recording's video, streamed from disk by the app: on its protocol, or on Linux from its video
   * server.
   */
  protected override streamUrl(id: string): string {
    const base = (window as DesktopWindow).aimviewVideoBase ?? DESKTOP_API;
    return `${base}/video?id=${encodeURIComponent(id)}`;
  }

  /**
   * Sends a file to /api/upload in parts of PART_BYTES (size=, at=, then upload=), which the service
   * joins; a file of one part goes whole. The top bar follows the parts sent. Gives the last part's
   * answer: the recording's id.
   */
  protected override async send(
    body: Blob,
    params: UploadParams,
    label: string,
  ): Promise<Uploaded> {
    if (body.size <= PART_BYTES) return super.send(body, params, label);
    const named: UploadParams = { ...params, size: String(body.size) };
    for (let at = 0; ; at += PART_BYTES) {
      this.show(label, at / body.size);
      const part = body.slice(at, at + PART_BYTES);
      const query = { ...named, at: String(at) };
      const answer = await firstValueFrom(
        this.client.post<Uploaded | UploadedPart>('/api/upload', part, { params: query }),
      );
      if (at + part.size >= body.size) return answer as Uploaded;
      named['upload'] = (answer as UploadedPart).upload;
    }
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
