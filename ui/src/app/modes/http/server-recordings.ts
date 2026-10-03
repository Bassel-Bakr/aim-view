import { HttpClient, HttpEventType, httpResource, HttpResponse } from '@angular/common/http';
import { computed, inject, Injectable, signal } from '@angular/core';
import { filter, lastValueFrom, map, tap } from 'rxjs';
import { Recording, Uploaded } from '../../api';
import {
  AddResult,
  FolderAction,
  RecordingSource,
  Transfer,
  VideoState,
} from '../../platform/recording-source';
import { readStats } from '../web-files/local-files';
import { statsForVideo, StatsCsv } from '../web-files/stats-csv';
import { isCsv, isVideo, mp4Name, toMp4 } from '../web-files/video-files';

/** The query of an upload: the file's name, and for a stats file the recording it is for. */
export type UploadParams = Record<string, string>;

/**
 * The review server's recordings (/api/vods), streamed from it (/video). Files added are sent to it and kept there
 * (/api/upload): a video that is not an MP4 is remuxed into one in the browser first, so every browser plays it.
 */
@Injectable({ providedIn: 'root' })
export class ServerRecordings implements RecordingSource {
  private readonly http = inject(HttpClient);
  protected readonly list = httpResource<Recording[]>(() => '/api/vods');
  readonly recordings = computed<Recording[]>(() =>
    this.list.hasValue() ? this.list.value() : [],
  );
  readonly loading = computed(() => this.list.isLoading() && !this.list.hasValue());
  readonly problem = computed<string | null>(() =>
    this.list.error() ? 'The review server is not running. Start it with bun run server.' : null,
  );
  readonly addedFilesGo: string = 'They are sent to the review server, which keeps them.';
  readonly transfer = signal<Transfer | null>(null);
  /** The server lists its own recordings folder. */
  readonly folder = signal<FolderAction | null>(null).asReadonly();
  readonly clearable = false;

  video(id: string): VideoState {
    return { state: 'ready', url: `/video?id=${encodeURIComponent(id)}`, remuxed: false };
  }

  lasting(): boolean {
    return true;
  }

  /** Sends the videos one by one, each with its stats file when one among the .csv files matches it. */
  async add(files: readonly File[]): Promise<AddResult> {
    const videos = files.filter(isVideo);
    const csvs = files.filter(isCsv);
    const read = await Promise.all(csvs.map(readStats));
    const stats = read.filter((s): s is StatsCsv => s !== null);
    const ids: string[] = [];
    try {
      for (const video of videos) {
        const mp4 = await toMp4(video, (share) =>
          this.show(`Remuxing ${video.name} into MP4`, share),
        );
        const name = mp4Name(video);
        const { id } = await this.send(mp4, { name }, `Uploading ${name}`);
        ids.push(id);
        const match = statsForVideo(video.name, stats, videos.length === 1);
        const csv = match && csvs[read.indexOf(match)];
        if (csv) await this.send(csv, { name: csv.name, id }, `Uploading ${csv.name}`);
      }
    } finally {
      this.transfer.set(null);
      this.list.reload();
    }
    return { ids, notStats: csvs.filter((_, i) => read[i] === null).map((f) => f.name) };
  }

  patch(id: string, change: Partial<Recording>): void {
    this.list.update((list) => list?.map((r) => (r.id === id ? { ...r, ...change } : r)));
  }

  /** The server's library is the recordings folder itself: it is not cleared from the page. */
  async clear(): Promise<void> {
    throw new Error("The review server's recordings are its folder's: they are not cleared here");
  }

  /** Sends one file to /api/upload; the top bar follows its progress (this mode's HttpClient reports it). */
  private send(body: Blob, params: UploadParams, label: string): Promise<Uploaded> {
    return lastValueFrom(
      this.http
        .post<Uploaded>('/api/upload', body, { params, reportProgress: true, observe: 'events' })
        .pipe(
          tap((e) => {
            if (e.type === HttpEventType.UploadProgress)
              this.show(label, e.total ? e.loaded / e.total : null);
          }),
          filter((e): e is HttpResponse<Uploaded> => e.type === HttpEventType.Response),
          map((e) => e.body as Uploaded),
        ),
    );
  }

  /** Shows what is being sent, in whole percents (each change redraws the top bar). */
  private show(label: string, share: number | null): void {
    const rounded = share === null ? null : Math.floor(100 * share) / 100;
    const now = this.transfer();
    if (now?.label !== label || now.share !== rounded) this.transfer.set({ label, share: rounded });
  }
}
