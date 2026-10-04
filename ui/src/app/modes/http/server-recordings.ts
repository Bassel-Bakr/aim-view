import { HttpClient, HttpEventType, httpResource, HttpResponse } from '@angular/common/http';
import { computed, inject, Injectable, Signal, signal, WritableSignal } from '@angular/core';
import { filter, firstValueFrom, lastValueFrom, map, tap } from 'rxjs';
import { errorMessage, Job, JobStage, LinkAdded, LinkInfo, Recording, Uploaded } from '../../api';
import {
  AddResult,
  FolderAction,
  RecordingSource,
  Transfer,
  VideoState,
} from '../../platform/recording-source';
import { readStats, statsForVideo, StatsCsv } from '../web-files/stats-csv';
import { isCsv, isVideo, mp4Name, toMp4 } from '../web-files/video-files';

/** The query of an upload: the file's name, and for a stats file the recording it is for. */
export type UploadParams = Record<string, string>;

/** A recording added from a link, as the page follows its download: its row until the list has it, and its video. */
export interface LinkDownload {
  row: Recording;
  video: VideoState;
}

/** A video sent to the service: the recording's id, and the video as sent (an MP4). */
export interface SentVideo {
  id: string;
  video: Blob;
}

/** How often a link's download is asked after, in milliseconds. */
const POLL_MS = 500;

/** What the server is doing for a link, by its job's stage. */
const LINK_STAGES: Partial<Record<JobStage, string>> = {
  'yt-dlp': 'Getting yt-dlp (once)',
  ffmpeg: 'Getting FFmpeg (once)',
  downloading: 'Downloading the video',
};

/**
 * The review server's recordings (/api/vods), streamed from it (/video). Files added are sent to it and kept there
 * (/api/upload): a video that is not an MP4 is remuxed into one in the browser first, so every browser plays it. A
 * link is downloaded by the server (/api/link, followed with /api/job), and listed here until its file is in.
 */
@Injectable({ providedIn: 'root' })
export class ServerRecordings implements RecordingSource {
  private readonly http = inject(HttpClient);
  protected readonly list = httpResource<Recording[]>(() => '/api/vods');
  protected readonly links = signal<ReadonlyMap<string, LinkDownload>>(new Map());
  readonly recordings = computed<Recording[]>(() => {
    const listed = this.list.hasValue() ? this.list.value() : [];
    const ids = new Set(listed.map((r) => r.id));
    const coming = [...this.links().values()].map((d) => d.row).filter((r) => !ids.has(r.id));
    return [...coming.reverse(), ...listed];
  });
  readonly loading = computed(() => this.list.isLoading() && !this.list.hasValue());
  readonly problem: Signal<string | null> = computed(() =>
    this.list.error() ? 'The review server is not running. Start it with bun run server.' : null,
  );
  readonly addedFilesGo: string = 'They are sent to the review server, which keeps them.';
  /** The files being sent, for the top bar. */
  protected readonly sending = signal<Transfer | null>(null);
  readonly transfer: Signal<Transfer | null> = this.sending.asReadonly();
  /** The server lists its own recordings folder. */
  readonly folder: Signal<FolderAction | null> = signal<FolderAction | null>(null).asReadonly();
  readonly clearable: boolean = false;
  /** The server downloads links itself. */
  readonly linkServer: WritableSignal<string> | null = null;

  video(id: string): VideoState | null {
    return this.links().get(id)?.video ?? this.ready(id);
  }

  /** Where the player streams a recording from. */
  protected streamUrl(id: string): string {
    return `/video?id=${encodeURIComponent(id)}`;
  }

  private ready(id: string): VideoState {
    return { state: 'ready', url: this.streamUrl(id), remuxed: false };
  }

  linkInfo(url: string): Promise<LinkInfo> {
    return firstValueFrom(this.http.post<LinkInfo>('/api/link/formats', { url }));
  }

  /** The server starts the download and names the recording; its row shows here until the list has the file. */
  async addLink(url: string, format: string | null): Promise<string> {
    const added = await firstValueFrom(this.http.post<LinkAdded>('/api/link', { url, format }));
    const video: VideoState = {
      state: 'downloading',
      label: LINK_STAGES.downloading ?? '',
      done: 0,
      total: 0,
    };
    this.setLink(added.id, { row: added.recording, video });
    void this.followLink(added.id);
    return added.id;
  }

  /** Follows a link's download until the video is in (its job is gone) or it failed. */
  private async followLink(id: string): Promise<void> {
    const link = this.links().get(id);
    if (!link) return;
    try {
      for (;;) {
        const job = await firstValueFrom(this.http.get<Job>('/api/job', { params: { id } }));
        if (job.stage === 'error') {
          this.setLink(id, { ...link, video: { state: 'not-downloaded', error: job.error ?? '' } });
          return;
        }
        if (job.stage === 'none' || !job.link) break;
        const label = LINK_STAGES[job.stage] ?? LINK_STAGES.downloading ?? '';
        const video: VideoState = {
          state: 'downloading',
          label,
          done: job.done ?? 0,
          total: job.total ?? 0,
        };
        this.setLink(id, { ...link, video });
        await new Promise((r) => setTimeout(r, POLL_MS));
      }
      this.setLink(id, { ...link, video: this.ready(id) });
      this.list.reload();
    } catch (e) {
      this.setLink(id, { ...link, video: { state: 'not-downloaded', error: errorMessage(e) } });
    }
  }

  protected setLink(id: string, download: LinkDownload): void {
    this.links.update((all) => new Map(all).set(id, download));
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
        const { id } = await this.sendVideo(video);
        ids.push(id);
        const match = statsForVideo(video.name, stats, videos.length === 1);
        const csv = match && csvs[read.indexOf(match)];
        if (csv) await this.send(csv, { name: csv.name, id }, `Uploading ${csv.name}`);
      }
    } finally {
      this.sending.set(null);
      this.list.reload();
    }
    return { ids, notStats: csvs.filter((_, i) => read[i] === null).map((f) => f.name) };
  }

  /** Sends a video, remuxed into MP4 in the browser first when it is not one. */
  protected async sendVideo(video: File): Promise<SentVideo> {
    const mp4 = await toMp4(video, (share) => this.show(`Remuxing ${video.name} into MP4`, share));
    const name = mp4Name(video);
    const { id } = await this.send(mp4, { name }, `Uploading ${name}`);
    return { id, video: mp4 };
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
    const now = this.sending();
    if (now?.label !== label || now.share !== rounded) this.sending.set({ label, share: rounded });
  }
}
