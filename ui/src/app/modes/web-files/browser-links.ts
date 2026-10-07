/**
 * Brings a video from a link into browser mode. In: a link the user pastes, and the Aim View
 * server on this computer where the browser cannot read the link itself. Out: the link's title and
 * qualities, and its video as a File with its progress, for browser-recordings.ts to add.
 */

import { HttpClient, HttpErrorResponse, HttpEventType, HttpResponse } from '@angular/common/http';
import { effect, inject, Service, signal } from '@angular/core';
import { filter, firstValueFrom, fromEvent, lastValueFrom, takeUntil, tap } from 'rxjs';
import { CANCELLED, Job, JobStage, LinkAdded, LinkInfo } from '../../api';

/** Where the browser keeps the address of the server that downloads links for it. */
const SERVER_KEY = 'link-server';
/** The Aim View server on this computer, as `bun run server` starts it. */
export const DEFAULT_LINK_SERVER = 'http://127.0.0.1:8770';
/** The error when the link server does not answer at all, saying how to start it. */
export const NO_LINK_SERVER =
  'Start the Aim View server (bun run server) to add from a link in this browser.';
/** A link path that names a video file, by its extension. */
const VIDEO_PATH = /\.(mp4|webm|mkv|mov)$/i;
/** How often the server's download is asked after, in milliseconds. */
const POLL_MS = 500;
/** The progress label while a video downloads, by the browser or the server. */
const DOWNLOADING = 'Downloading the video';
/** The progress label while the server's finished file is copied into the browser. */
const COPYING = 'Copying the video into this browser';
/** What the server is doing for a link, by its job's stage. */
const SERVER_STAGES: Partial<Record<JobStage, string>> = {
  'yt-dlp': 'The server is getting yt-dlp (once)',
  ffmpeg: 'The server is getting FFmpeg (once)',
  downloading: DOWNLOADING,
};

/**
 * Hears how far a link's video has come: what is being done, and the megabytes done of total (0:
 * not known).
 */
export type LinkProgress = (label: string, done: number, total: number) => void;

/**
 * A link's video on its way into this browser: its file name, the file once all of it is here, and
 * what stops it (the file then fails with CANCELLED).
 */
export interface LinkFetch {
  /** The file name the video gets: the link path's last part, or the name the server saved. */
  name: string;
  /** The whole video, once it is here; rejects with CANCELLED after cancel. */
  file: Promise<File>;
  /** Stops the download (and the server's, where it downloads). */
  cancel: () => void;
}

/** Whole megabytes (2^20 bytes) in a byte count, rounded down. */
const megabytes = (bytes: number) => Math.floor(bytes / 2 ** 20);

/** The last part of a link's path, as a file name. */
function fileName(url: string): string {
  const last = new URL(url).pathname.split('/').pop() ?? '';
  try {
    return decodeURIComponent(last) || 'video.mp4';
  } catch {
    return last || 'video.mp4';
  }
}

/** The link server's address the browser kept, else the default (also where storage is blocked). */
function readServer(): string {
  try {
    return localStorage.getItem(SERVER_KEY) ?? DEFAULT_LINK_SERVER;
  } catch {
    return DEFAULT_LINK_SERVER;
  }
}

/**
 * Links in browser mode. The browser downloads a video file's address itself when its host lets
 * other sites read it. Any other link (YouTube, Twitch, Medal...) the browser cannot read: the Aim
 * View server on this computer downloads it with yt-dlp (/api/link, followed with /api/job), and
 * the page copies the finished file from it (/video) into the browser.
 */
@Service()
export class BrowserLinks {
  /** Sends the HEAD checks, the downloads and the link server's requests. */
  private readonly http = inject(HttpClient);
  /** The server's address, which the user can change; the browser keeps it. */
  readonly server = signal(readServer());
  /** The links the browser reads itself. */
  private readonly direct = new Set<string>();

  /** Keeps the server's address in localStorage each time the user changes it. */
  constructor() {
    effect(() => {
      const address = this.server();
      try {
        localStorage.setItem(SERVER_KEY, address);
      } catch {
        // the address is kept for this visit only
      }
    });
  }

  /**
   * What the link offers: a video file the browser reads has nothing to choose; else the server
   * reads it.
   */
  async info(url: string): Promise<LinkInfo> {
    if (await this.readsItself(url)) return { title: fileName(url), duration: null, formats: [] };
    return this.ask<LinkInfo>('/api/link/formats', { url });
  }

  /**
   * Starts bringing the link's video into the browser, in the chosen quality where the server
   * downloads it (format null: the best). Resolves once it has started.
   */
  async start(url: string, format: string | null, progress: LinkProgress): Promise<LinkFetch> {
    const stop = new AbortController();
    const cancel = () => stop.abort();
    if (await this.readsItself(url)) {
      const name = fileName(url);
      return { name, file: this.download(url, name, progress, stop.signal), cancel };
    }
    const added = await this.ask<LinkAdded>('/api/link', { url, format });
    return { name: added.saved, file: this.copy(added, progress, stop.signal), cancel };
  }

  /**
   * Whether the browser reads the link itself: its host lets other sites read it (a HEAD request
   * gets an answer, even an error status), and it is a video file (its path's extension, or the
   * answer's type). A link found readable is remembered for this visit.
   */
  private async readsItself(url: string): Promise<boolean> {
    if (this.direct.has(url)) return true;
    const head = await firstValueFrom(this.http.head(url, { observe: 'response' })).catch(
      (error: unknown) => (error instanceof HttpErrorResponse && error.status !== 0 ? error : null),
    );
    const type = head?.headers.get('content-type') ?? '';
    const reads =
      head !== null && (VIDEO_PATH.test(new URL(url).pathname) || type.startsWith('video/'));
    if (reads) this.direct.add(url);
    return reads;
  }

  /** Downloads a video file in the browser; rejects with CANCELLED when `stop` is aborted. */
  private async download(
    url: string,
    name: string,
    progress: LinkProgress,
    stop: AbortSignal,
  ): Promise<File> {
    const done = await lastValueFrom(
      this.http.get(url, { responseType: 'blob', observe: 'events', reportProgress: true }).pipe(
        takeUntil(fromEvent(stop, 'abort')),
        tap((event) => {
          if (event.type === HttpEventType.DownloadProgress)
            progress(DOWNLOADING, megabytes(event.loaded), megabytes(event.total ?? 0));
        }),
        filter((event): event is HttpResponse<Blob> => event.type === HttpEventType.Response),
      ),
      { defaultValue: null },
    );
    if (!done) throw new Error(CANCELLED);
    const body = done.body ?? new Blob();
    return new File([body], name, { type: body.type || 'video/mp4' });
  }

  /**
   * Follows the server's download until the video is in, then copies it into the browser a range
   * at a time; stopped (and the server's download cancelled) when `stop` is aborted.
   */
  private async copy(added: LinkAdded, progress: LinkProgress, stop: AbortSignal): Promise<File> {
    await this.downloaded(added, progress, stop);
    const parts: Blob[] = [];
    let at = 0;
    for (;;) {
      if (stop.aborted) throw new Error(CANCELLED);
      const answer = await this.range(added.id, at);
      const size = Number(/\/(\d+)$/.exec(answer.headers.get('content-range') ?? '')?.[1] ?? 0);
      const body = answer.body ?? new Blob();
      if (body.size) parts.push(body);
      at += body.size;
      progress(COPYING, megabytes(at), megabytes(size));
      if (!body.size || at >= size) break;
    }
    return new File(parts, added.saved, { type: 'video/mp4' });
  }

  /**
   * Waits for the server's download of a link, showing how far it is; cancels it there when `stop`
   * is aborted, and rejects when the server's download failed.
   */
  private async downloaded(
    added: LinkAdded,
    progress: LinkProgress,
    stop: AbortSignal,
  ): Promise<void> {
    for (;;) {
      if (stop.aborted) {
        await this.ask<Job>('/api/cancel', {}, { id: added.id }).catch(() => undefined);
        throw new Error(CANCELLED);
      }
      const job = await this.ask<Job>('/api/job', undefined, { id: added.id });
      if (job.stage === 'error') throw new Error(job.error ?? 'the download failed');
      if (job.stage === 'none' || !job.link) return;
      const label = SERVER_STAGES[job.stage] ?? DOWNLOADING;
      progress(label, job.done ?? 0, job.total ?? 0);
      await new Promise((resolve) => setTimeout(resolve, POLL_MS));
    }
  }

  /** The server's video from byte `at` on (it answers at most a few megabytes at a time). */
  private range(id: string, at: number): Promise<HttpResponse<Blob>> {
    const url = `${this.base()}/video`;
    return firstValueFrom(
      this.http.get(url, {
        params: { id },
        headers: { Range: `bytes=${at}-` },
        responseType: 'blob',
        observe: 'response',
      }),
    ).catch((error: unknown) => {
      throw this.plain(error);
    });
  }

  /** Asks the server: with a body, a POST; without, a GET; either with `params` in its query. */
  private ask<T>(path: string, body?: object, params?: Record<string, string>): Promise<T> {
    const url = `${this.base()}${path}`;
    const sent =
      body === undefined
        ? this.http.get<T>(url, { params })
        : this.http.post<T>(url, body, { params });
    return firstValueFrom(sent).catch((error: unknown) => {
      throw this.plain(error);
    });
  }

  /** No answer at all means no server: say how to start it. */
  private plain(error: unknown): unknown {
    return error instanceof HttpErrorResponse && error.status === 0
      ? new Error(NO_LINK_SERVER)
      : error;
  }

  /** The server's address as the user typed it, without spaces or a trailing slash. */
  private base(): string {
    return this.server().trim().replace(/\/+$/, '');
  }
}
