/**
 * Server mode's `RecordingSource`, which the desktop and browser modes extend. In: the review
 * service's API (/api/vods, /api/upload, /api/link, /api/job) and the files the user adds. Out: the
 * recordings list and each video's state for the recordings page, the run page and the top bar.
 */

import { HttpClient, HttpEventType, httpResource, HttpResponse } from '@angular/common/http';
import { computed, inject, Service, Signal, signal, WritableSignal } from '@angular/core';
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

/**
 * A recording added from a link, as the page follows its download: its row until the list has
 * it, and its video.
 */
export interface LinkDownload {
  /** The recording's row as the service named it when the download started. */
  row: Recording;
  /** How far the download is, then ready (or not downloaded, and why). */
  video: VideoState;
}

/** A video sent to the service: the recording's id, and the video as sent (an MP4). */
export interface SentVideo {
  /** The recording's id the service gave the upload. */
  id: string;
  /** The video as sent: the user's file, or its MP4 remux. */
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
 * The review server's recordings (/api/vods), streamed from it (/video). Files added are sent to
 * it and kept there (/api/upload): a video that is not an MP4 is remuxed into one in the browser
 * first, so every browser plays it. The server downloads a link (/api/link, followed with
 * /api/job), and the page lists it until its file is in.
 */
@Service()
export class ServerRecordings implements RecordingSource {
  /** Sends the uploads and the link requests. */
  private readonly http = inject(HttpClient);
  /**
   * The list from the recordings' names alone, which the server gives at once
   * (/api/vods?quick=1), shown until the whole list (each recording's stats file, review and kind)
   * is in. Asked first, so it comes first.
   */
  private readonly quickList = httpResource<Recording[]>(() => ({
    url: '/api/vods',
    params: { quick: '1' },
  }));
  /** The whole list (/api/vods); reloaded after an upload, a finished link or a new folder. */
  protected readonly list = httpResource<Recording[]>(() => '/api/vods');
  /** The links added from this page, by recording id, while they download and after. */
  protected readonly links = signal<ReadonlyMap<string, LinkDownload>>(new Map());
  /** The listed recordings, after the rows of links the list lacks yet (newest link first). */
  readonly recordings = computed<Recording[]>(() => {
    const listed = this.list.hasValue()
      ? this.list.value()
      : this.quickList.hasValue()
        ? this.quickList.value()
        : [];
    const ids = new Set(listed.map((recording) => recording.id));
    const coming = [...this.links().values()]
      .map((link) => link.row)
      .filter((row) => !ids.has(row.id));
    return [...coming.reverse(), ...listed];
  });
  /** True until either list first comes in. */
  readonly loading = computed(
    () => this.list.isLoading() && !this.list.hasValue() && !this.quickList.hasValue(),
  );
  /** The list's error: here it means the review server is not running. */
  readonly problem: Signal<string | null> = computed(() =>
    this.list.error() ? 'The review server is not running. Start it with bun run server.' : null,
  );
  /** Where files the user adds end up, as the upload button and the empty page say. */
  readonly addedFilesGo: string = 'They are sent to the review server, which keeps them.';
  /** The files being sent, for the top bar. */
  protected readonly sending = signal<Transfer | null>(null);
  /** The file being sent and its share done, for the top bar; null when nothing is sent. */
  readonly transfer: Signal<Transfer | null> = this.sending.asReadonly();
  /** The server lists its own recordings folder. */
  readonly folder: Signal<FolderAction | null> = signal<FolderAction | null>(null).asReadonly();
  /** The server's library is its folder, so the page cannot clear it. */
  readonly clearable: boolean = false;
  /** The server downloads links itself. */
  readonly linkServer: WritableSignal<string> | null = null;

  /** A link's video while it downloads (or failed to), else the video streamed from the server. */
  video(id: string): VideoState | null {
    return this.links().get(id)?.video ?? this.ready(id);
  }

  /** The recording's video as the service keeps it, read from where the player streams it. */
  async videoFile(id: string): Promise<Blob> {
    const response = await fetch(this.streamUrl(id));
    if (!response.ok) throw new Error(`The video of ${id} could not be read (${response.status})`);
    return response.blob();
  }

  /** Where the player streams a recording from. */
  protected streamUrl(id: string): string {
    return `/video?id=${encodeURIComponent(id)}`;
  }

  /** The recording's video, ready to stream as the server keeps it (never remuxed here). */
  private ready(id: string): VideoState {
    return { state: 'ready', url: this.streamUrl(id), remuxed: false };
  }

  /** The link's title and qualities, as the server's yt-dlp reads them (POST /api/link/formats). */
  linkInfo(url: string): Promise<LinkInfo> {
    return firstValueFrom(this.http.post<LinkInfo>('/api/link/formats', { url }));
  }

  /**
   * The server starts the download and names the recording; its row shows here until the list has
   * the file. Gives the new recording's id.
   */
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

  /** Asks the server to cancel the link's job; `followLink` then marks the video cancelled. */
  async cancelLink(id: string): Promise<void> {
    await firstValueFrom(this.http.post<Job>('/api/cancel', null, { params: { id } }));
  }

  /**
   * Follows a link's download every `POLL_MS` until the video is in (its job is gone), it failed,
   * or it was cancelled.
   */
  private async followLink(id: string): Promise<void> {
    const link = this.links().get(id);
    if (!link) return;
    try {
      for (;;) {
        const job = await firstValueFrom(this.http.get<Job>('/api/job', { params: { id } }));
        if (job.stage === 'error' || job.stage === 'cancelled') {
          const error = job.stage === 'cancelled' ? 'Cancelled' : (job.error ?? '');
          this.setLink(id, { ...link, video: { state: 'not-downloaded', error } });
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
        await new Promise((resolve) => setTimeout(resolve, POLL_MS));
      }
      this.setLink(id, { ...link, video: this.ready(id) });
      this.list.reload();
    } catch (error) {
      this.setLink(id, { ...link, video: { state: 'not-downloaded', error: errorMessage(error) } });
    }
  }

  /** Records a link's row and video state, in a new map so the signal's readers update. */
  protected setLink(id: string, download: LinkDownload): void {
    this.links.update((all) => new Map(all).set(id, download));
  }

  /** Always true: the server keeps every recording, so its id opens it after a reload. */
  lasting(): boolean {
    return true;
  }

  /**
   * Sends the videos one by one, each with its stats file when one among the .csv files matches it.
   * Gives the new recordings' ids and the .csv files that are not stats files.
   */
  async add(files: readonly File[]): Promise<AddResult> {
    const videos = files.filter(isVideo);
    const csvs = files.filter(isCsv);
    const read = await Promise.all(csvs.map(readStats));
    const stats = read.filter((stats): stats is StatsCsv => stats !== null);
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
    return { ids, notStats: csvs.filter((_csv, i) => read[i] === null).map((file) => file.name) };
  }

  /** Sends a video, remuxed into MP4 in the browser first when it is not one. */
  protected async sendVideo(video: File): Promise<SentVideo> {
    const mp4 = await toMp4(video, (share) => this.show(`Remuxing ${video.name} into MP4`, share));
    const name = mp4Name(video);
    const { id } = await this.send(mp4, { name }, `Uploading ${name}`);
    return { id, video: mp4 };
  }

  /** Changes the recording's row in the loaded list, without asking the server again. */
  patch(id: string, change: Partial<Recording>): void {
    this.list.update((list) =>
      list?.map((recording) => (recording.id === id ? { ...recording, ...change } : recording)),
    );
  }

  /** Always rejects: the server's library is its recordings folder, which the page cannot clear. */
  async clear(): Promise<void> {
    throw new Error("The review server's recordings are its folder's: they are not cleared here");
  }

  /**
   * Sends one file to /api/upload; the top bar follows its progress (this mode's HttpClient reports
   * it). Gives the server's answer: the recording's id.
   */
  private send(body: Blob, params: UploadParams, label: string): Promise<Uploaded> {
    return lastValueFrom(
      this.http
        .post<Uploaded>('/api/upload', body, { params, reportProgress: true, observe: 'events' })
        .pipe(
          tap((event) => {
            if (event.type === HttpEventType.UploadProgress)
              this.show(label, event.total ? event.loaded / event.total : null);
          }),
          filter((event): event is HttpResponse<Uploaded> => event.type === HttpEventType.Response),
          map((response) => response.body as Uploaded),
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
