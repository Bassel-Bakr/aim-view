import { computed, inject, Injectable, signal, WritableSignal } from '@angular/core';
import { errorMessage, Recording, StatsHow } from '../../api';
import { AddResult, RecordingSource, Transfer, VideoState } from '../../platform/recording-source';
import {
  parseStatsCsv,
  parseVodName,
  stampSeconds,
  statsForVideo,
  StatsCsv,
  statsSummary,
} from './stats-csv';
import { StatsFolder } from './stats-folder';
import { isCsv, isMp4, isVideo, toMp4 } from './video-files';

const LOCAL = 'local:';

/**
 * A recording opened from this computer, with its stats file when it has one, and changes made to its row since.
 * A video that is not an MP4 is remuxed into one in the browser first. It stays in this browser: nothing is sent or
 * saved, and it is gone when the page closes.
 */
export interface LocalFile {
  id: string;
  file: File;
  video: WritableSignal<VideoState>;
  stats: StatsCsv | null;
  /** How the stats file came to it: chosen by the user (upload, picked), found by name and time, or none. */
  statsHow: StatsHow;
  added: number;
  changes: Partial<Recording>;
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
    analysed: false,
    not_aim: false,
    local: true,
    ...f.changes,
    stats: f.stats !== null,
  };
}

/** Reads a .csv file as a stats file, or null when it is not one. */
export async function readStats(file: File): Promise<StatsCsv | null> {
  return parseStatsCsv(file.name, await file.text());
}

/**
 * The recordings opened from this computer, newest first, each played from the browser's own copy. Remuxes run one at
 * a time, so two large videos are never in memory at once.
 */
@Injectable({ providedIn: 'root' })
export class LocalFiles implements RecordingSource {
  private readonly folder = inject(StatsFolder);
  readonly files = signal<LocalFile[]>([]);
  readonly recordings = computed<Recording[]>(() => this.files().map(localRecording));
  readonly loading = signal(false).asReadonly();
  readonly problem = signal<string | null>(null).asReadonly();
  readonly addedFilesGo = 'They stay in this browser.';
  readonly transfer = signal<Transfer | null>(null);
  private count = 0;
  private remuxes: Promise<void> = Promise.resolve();

  video(id: string): VideoState | null {
    return this.find(id)?.video() ?? null;
  }

  lasting(): boolean {
    return false;
  }

  /** Opens the videos among files, each with its stats file among the .csv files when one matches it. */
  async add(files: readonly File[]): Promise<AddResult> {
    const videos = files.filter(isVideo);
    const csvs = files.filter(isCsv);
    const read = await Promise.all(csvs.map(readStats));
    const stats = read.filter((s): s is StatsCsv => s !== null);
    const added = videos.map((file): LocalFile => ({
      id: `${LOCAL}${++this.count}/${file.name}`,
      file,
      video: signal<VideoState>(
        isMp4(file)
          ? { state: 'ready', url: URL.createObjectURL(file), remuxed: false }
          : { state: 'remuxing', progress: 0 },
      ),
      stats: statsForVideo(file.name, stats, videos.length === 1),
      statsHow: 'missing',
      added: Date.now(),
      changes: {},
    }));
    for (const f of added) f.statsHow = f.stats ? 'upload' : 'missing';
    this.files.update((list) => [...[...added].reverse(), ...list]);
    for (const f of added) {
      if (f.video().state === 'remuxing') this.remuxes = this.remuxes.then(() => this.remux(f));
    }
    await Promise.all(added.filter((f) => !f.stats).map((f) => this.findStats(f.id)));
    return {
      ids: added.map((f) => f.id),
      notStats: csvs.filter((_, i) => read[i] === null).map((f) => f.name),
    };
  }

  patch(id: string, change: Partial<Recording>): void {
    this.update(id, (f) => ({ ...f, changes: { ...f.changes, ...change } }));
  }

  /** Pairs a recording with a stats file; false when the file is not one of KovaaK's stats files. */
  async pair(id: string, file: File, how: StatsHow = 'upload'): Promise<boolean> {
    const stats = await readStats(file);
    if (stats) this.update(id, (f) => ({ ...f, stats, statsHow: how }));
    return stats !== null;
  }

  /** The user says the recording has no stats file. */
  unpair(id: string): void {
    this.update(id, (f) => ({ ...f, stats: null, statsHow: 'none' }));
  }

  /**
   * Finds the recording's stats file in KovaaK's stats folder by its scenario and time (within five seconds), as the
   * review server does; with the folder not open, or no such file, it has none. Returns whether it found one.
   */
  async findStats(id: string): Promise<boolean> {
    const f = this.find(id);
    if (!f) return false;
    const r = localRecording(f);
    const entry = this.folder.ready() ? this.folder.find(r.scenario, stampSeconds(r.stamp)) : null;
    if (!entry) {
      this.update(id, (g) => ({ ...g, stats: null, statsHow: 'missing' }));
      return false;
    }
    return this.pair(id, await this.folder.read(entry.name), 'found');
  }

  /** Finds the stats file of every recording that has none and was not told it has none (the folder just opened). */
  async findAllStats(): Promise<void> {
    const open = this.files().filter((f) => f.statsHow === 'missing');
    await Promise.all(open.map((f) => this.findStats(f.id)));
  }

  find(id: string | null): LocalFile | null {
    return this.files().find((f) => f.id === id) ?? null;
  }

  /** Remuxes a file into MP4, showing the progress in whole percents (each change redraws what shows it). */
  private async remux(f: LocalFile): Promise<void> {
    const label = `Remuxing ${f.file.name} into MP4`;
    try {
      const mp4 = await toMp4(f.file, (share) => {
        const progress = Math.floor(100 * share) / 100;
        const now = f.video();
        if (now.state === 'remuxing' && now.progress === progress) return;
        f.video.set({ state: 'remuxing', progress });
        this.transfer.set({ label, share: progress });
      });
      f.video.set({ state: 'ready', url: URL.createObjectURL(mp4), remuxed: true });
    } catch (e) {
      f.video.set({ state: 'failed', url: URL.createObjectURL(f.file), error: errorMessage(e) });
    } finally {
      this.transfer.set(null);
    }
  }

  private update(id: string, change: (f: LocalFile) => LocalFile): void {
    this.files.update((list) => list.map((f) => (f.id === id ? change(f) : f)));
  }
}
