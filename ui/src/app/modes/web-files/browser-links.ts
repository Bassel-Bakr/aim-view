import { HttpClient, HttpErrorResponse, HttpEventType, HttpResponse } from '@angular/common/http';
import { effect, inject, Injectable, signal } from '@angular/core';
import { filter, firstValueFrom, lastValueFrom, tap } from 'rxjs';
import { Job, JobStage, LinkAdded, LinkInfo } from '../../api';

/** Where the browser keeps the address of the server that downloads links for it. */
const SERVER_KEY = 'link-server';
/** The Aim View server on this computer, as `bun run server` starts it. */
export const DEFAULT_LINK_SERVER = 'http://127.0.0.1:8770';
export const NO_LINK_SERVER =
  'Start the Aim View server (bun run server) to add from a link in this browser.';
const VIDEO_PATH = /\.(mp4|webm|mkv|mov)$/i;
const POLL_MS = 500;
const DOWNLOADING = 'Downloading the video';
const COPYING = 'Copying the video into this browser';
/** What the server is doing for a link, by its job's stage. */
const SERVER_STAGES: Partial<Record<JobStage, string>> = {
  'yt-dlp': 'The server is getting yt-dlp (once)',
  ffmpeg: 'The server is getting FFmpeg (once)',
  downloading: DOWNLOADING,
};

/** Hears how far a link's video has come: what is being done, and the megabytes done of total (0: not known). */
export type LinkProgress = (label: string, done: number, total: number) => void;

/** A link's video on its way into this browser: its file name, and the file once all of it is here. */
export interface LinkFetch {
  name: string;
  file: Promise<File>;
}

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

function readServer(): string {
  try {
    return localStorage.getItem(SERVER_KEY) ?? DEFAULT_LINK_SERVER;
  } catch {
    return DEFAULT_LINK_SERVER;
  }
}

/**
 * Links in browser mode. A video file's address whose host lets other sites read it is downloaded by the browser
 * itself. Any other link (YouTube, Twitch, Medal...) the browser cannot read: the Aim View server on this computer
 * downloads it with yt-dlp (/api/link, followed with /api/job), and the finished file is copied from it (/video) into
 * the browser.
 */
@Injectable({ providedIn: 'root' })
export class BrowserLinks {
  private readonly http = inject(HttpClient);
  /** The server's address, which the user can change; the browser keeps it. */
  readonly server = signal(readServer());
  /** The links the browser reads itself. */
  private readonly direct = new Set<string>();

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

  /** What the link offers: a video file the browser reads has nothing to choose; else the server reads it. */
  async info(url: string): Promise<LinkInfo> {
    if (await this.readsItself(url)) return { title: fileName(url), duration: null, formats: [] };
    return this.ask<LinkInfo>('/api/link/formats', { url });
  }

  /** Starts bringing the link's video into the browser, in the chosen quality where the server downloads it. */
  async start(url: string, format: string | null, progress: LinkProgress): Promise<LinkFetch> {
    if (await this.readsItself(url)) {
      const name = fileName(url);
      return { name, file: this.download(url, name, progress) };
    }
    const added = await this.ask<LinkAdded>('/api/link', { url, format });
    return { name: added.saved, file: this.copy(added, progress) };
  }

  /**
   * Whether the browser reads the link itself: its host lets other sites read it (a HEAD request gets an answer),
   * and it is a video file (its path's extension, or the answer's type).
   */
  private async readsItself(url: string): Promise<boolean> {
    if (this.direct.has(url)) return true;
    const head = await firstValueFrom(this.http.head(url, { observe: 'response' })).catch(
      (e: unknown) => (e instanceof HttpErrorResponse && e.status !== 0 ? e : null),
    );
    const type = head?.headers.get('content-type') ?? '';
    const reads =
      head !== null && (VIDEO_PATH.test(new URL(url).pathname) || type.startsWith('video/'));
    if (reads) this.direct.add(url);
    return reads;
  }

  /** Downloads a video file in the browser. */
  private async download(url: string, name: string, progress: LinkProgress): Promise<File> {
    const done = await lastValueFrom(
      this.http.get(url, { responseType: 'blob', observe: 'events', reportProgress: true }).pipe(
        tap((e) => {
          if (e.type === HttpEventType.DownloadProgress)
            progress(DOWNLOADING, megabytes(e.loaded), megabytes(e.total ?? 0));
        }),
        filter((e): e is HttpResponse<Blob> => e.type === HttpEventType.Response),
      ),
    );
    const body = done.body ?? new Blob();
    return new File([body], name, { type: body.type || 'video/mp4' });
  }

  /** Follows the server's download until the video is in, then copies it into the browser a range at a time. */
  private async copy(added: LinkAdded, progress: LinkProgress): Promise<File> {
    for (;;) {
      const job = await this.ask<Job>('/api/job', undefined, { id: added.id });
      if (job.stage === 'error') throw new Error(job.error ?? 'the download failed');
      if (job.stage === 'none' || !job.link) break;
      const label = SERVER_STAGES[job.stage] ?? DOWNLOADING;
      progress(label, job.done ?? 0, job.total ?? 0);
      await new Promise((r) => setTimeout(r, POLL_MS));
    }
    const parts: Blob[] = [];
    let at = 0;
    for (;;) {
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
    ).catch((e: unknown) => {
      throw this.plain(e);
    });
  }

  /** Asks the server: with a body, a POST; without, a GET. */
  private ask<T>(path: string, body?: object, params?: Record<string, string>): Promise<T> {
    const url = `${this.base()}${path}`;
    const sent =
      body === undefined ? this.http.get<T>(url, { params }) : this.http.post<T>(url, body);
    return firstValueFrom(sent).catch((e: unknown) => {
      throw this.plain(e);
    });
  }

  /** No answer at all means no server: say how to start it. */
  private plain(e: unknown): unknown {
    return e instanceof HttpErrorResponse && e.status === 0 ? new Error(NO_LINK_SERVER) : e;
  }

  private base(): string {
    return this.server().trim().replace(/\/+$/, '');
  }
}
