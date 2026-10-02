import { computed, Injectable, signal, WritableSignal } from '@angular/core';
import { errorMessage, Recording } from '../api';
import { parseStatsCsv, parseVodName, statsForVideo, StatsCsv, statsSummary } from './stats-csv';

const LOCAL = 'local:';
const VIDEO = /\.(mp4|mkv|mov|webm)$/i;
const CSV = /\.csv$/i;
const MP4 = /\.mp4$/i;

/** Whether a recording's id is one opened from this computer. */
export function isLocal(id: string | null): boolean {
  return id?.startsWith(LOCAL) ?? false;
}

/** A video being remuxed into MP4; progress is the share done, 0 to 1. */
export interface VideoRemuxing {
  state: 'remuxing';
  progress: number;
}

/** A video ready to play from url; remuxed when it was remuxed into MP4 in the browser. */
export interface VideoReady {
  state: 'ready';
  url: string;
  remuxed: boolean;
}

/** A video the remux failed on: it plays from the file as it is, if the browser can play it. */
export interface VideoFailed {
  state: 'failed';
  url: string;
  error: string;
}

export type LocalVideo = VideoRemuxing | VideoReady | VideoFailed;

/**
 * A recording opened from this computer, with its stats file when it has one. A video that is not an MP4 is remuxed
 * into one in the browser first (see remuxToMp4). It stays in this browser: nothing is sent or saved, and it is gone
 * when the page closes.
 */
export interface LocalFile {
  id: string;
  file: File;
  video: WritableSignal<LocalVideo>;
  stats: StatsCsv | null;
  added: number;
}

/** What adding files did: the recordings opened, and the .csv files that are not KovaaK's stats files. */
export interface LocalAdd {
  ids: string[];
  notStats: string[];
}

/** A local file as a row of the recordings list; the stats file gives what the video's name does not. */
export function localRecording(f: LocalFile): Recording {
  const vod = parseVodName(f.file.name);
  const stats = f.stats && statsSummary(f.stats);
  const stamp = new Date(f.file.lastModified);
  const pad = (n: number) => String(n).padStart(2, '0');
  return {
    id: f.id,
    scenario: vod?.scenario ?? stats?.scenario ?? f.file.name.replace(/\.\w+$/, ''),
    kind: null,
    score: vod?.score ?? stats?.score ?? null,
    stamp:
      vod?.stamp ??
      stats?.stamp ??
      `${stamp.getFullYear()}.${pad(stamp.getMonth() + 1)}.${pad(stamp.getDate())}-` +
        `${pad(stamp.getHours())}.${pad(stamp.getMinutes())}.${pad(stamp.getSeconds())}`,
    mtime: f.added / 1000,
    size: f.file.size,
    stats: f.stats !== null,
    analysed: false,
    not_aim: false,
    local: true,
  };
}

/** Reads a .csv file as a stats file, or null when it is not one. */
async function readStats(file: File): Promise<StatsCsv | null> {
  return parseStatsCsv(file.name, await file.text());
}

/**
 * The recordings opened from this computer, newest first, each played from the browser's own copy. Remuxes run one at
 * a time, so two large videos are never in memory at once.
 */
@Injectable({ providedIn: 'root' })
export class LocalFiles {
  readonly files = signal<LocalFile[]>([]);
  readonly recordings = computed<Recording[]>(() => this.files().map(localRecording));
  private count = 0;
  private remuxes: Promise<void> = Promise.resolve();

  /** Opens the videos among files, each with its stats file among the .csv files when one matches it. */
  async add(files: readonly File[]): Promise<LocalAdd> {
    const videos = files.filter((f) => VIDEO.test(f.name));
    const csvs = files.filter((f) => CSV.test(f.name));
    const read = await Promise.all(csvs.map(readStats));
    const stats = read.filter((s): s is StatsCsv => s !== null);
    const added = videos.map((file): LocalFile => ({
      id: `${LOCAL}${++this.count}/${file.name}`,
      file,
      video: signal<LocalVideo>(
        MP4.test(file.name)
          ? { state: 'ready', url: URL.createObjectURL(file), remuxed: false }
          : { state: 'remuxing', progress: 0 },
      ),
      stats: statsForVideo(file.name, stats, videos.length === 1),
      added: Date.now(),
    }));
    this.files.update((list) => [...[...added].reverse(), ...list]);
    for (const f of added) {
      if (f.video().state === 'remuxing') this.remuxes = this.remuxes.then(() => this.remux(f));
    }
    return {
      ids: added.map((f) => f.id),
      notStats: csvs.filter((_, i) => read[i] === null).map((f) => f.name),
    };
  }

  /** Pairs a recording with a stats file; false when the file is not one of KovaaK's stats files. */
  async pair(id: string, file: File): Promise<boolean> {
    const stats = await readStats(file);
    if (stats) this.setStats(id, stats);
    return stats !== null;
  }

  /** The recording has no stats file. */
  unpair(id: string): void {
    this.setStats(id, null);
  }

  find(id: string | null): LocalFile | null {
    return this.files().find((f) => f.id === id) ?? null;
  }

  /**
   * Remuxes a file into MP4, showing the progress in whole percents (each change redraws what shows it). The remux
   * code loads only now, the first time a video needs it.
   */
  private async remux(f: LocalFile): Promise<void> {
    try {
      const { remuxToMp4 } = await import('./remux');
      const mp4 = await remuxToMp4(f.file, (share) => {
        const progress = Math.floor(100 * share) / 100;
        if (progress !== (f.video() as VideoRemuxing).progress)
          f.video.set({ state: 'remuxing', progress });
      });
      f.video.set({ state: 'ready', url: URL.createObjectURL(mp4), remuxed: true });
    } catch (e) {
      f.video.set({ state: 'failed', url: URL.createObjectURL(f.file), error: errorMessage(e) });
    }
  }

  private setStats(id: string, stats: StatsCsv | null): void {
    this.files.update((list) => list.map((f) => (f.id === id ? { ...f, stats } : f)));
  }
}
