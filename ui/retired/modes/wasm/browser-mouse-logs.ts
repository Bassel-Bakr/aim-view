import { inject, Injectable, resource, ResourceRef } from '@angular/core';
import {
  MouseLoggerState,
  MouseMeasures,
  MouseReadOutcome,
  MouseReadRequest,
} from '../../mouse-api';
import { MouseLogs } from '../../platform/mouse-logs';
import { LocalFiles } from '../web-files/local-files';
import { SavedMouseLogs } from '../web-files/saved-mouse-logs';
import { StatsCsv } from '../web-files/stats-csv';
import { CoreModule } from './core-module';

/** What a recording's measures are read from: the recording, its stats file and the name of its kept log. */
export interface BrowserMouseParams {
  id: string;
  stats: StatsCsv | null;
  log: string | null;
}

/**
 * This computer's offset from UTC at the log's start (local minus UTC, s), from the start time in its header: the
 * stats file's times are local.
 */
export function logUtcOffset(log: Uint8Array): number {
  if (log.length < 32) return 0;
  const ns = new DataView(log.buffer, log.byteOffset, log.byteLength).getBigInt64(24, true);
  return -new Date(Number(ns / 1_000_000n)).getTimezoneOffset() * 60;
}

/**
 * Mouse logs in the browser: the user adds a log for a recording, the core reads it here with the recording's stats
 * file (src/mouse.rs, as WebAssembly), and the browser keeps it with the recording. Nothing is sent anywhere.
 */
@Injectable({ providedIn: 'root' })
export class BrowserMouseLogs implements MouseLogs {
  private readonly core = inject(CoreModule);
  private readonly local = inject(LocalFiles);
  private readonly saved = inject(SavedMouseLogs);
  readonly adds = true;
  readonly logs = false;

  measures(id: () => string | undefined): ResourceRef<MouseMeasures | null | undefined> {
    return resource<MouseMeasures | null, BrowserMouseParams | undefined>({
      params: () => {
        const f = this.local.find(id() ?? null);
        return f ? { id: f.id, stats: f.stats, log: this.saved.name(f.file) } : undefined;
      },
      loader: async ({ params }) => {
        const f = this.local.find(params.id);
        if (!f || !params.stats || !params.log) return null;
        const log = await this.saved.load(f.file);
        return log ? this.read(new Uint8Array(log), params.stats, params.log) : null;
      },
    });
  }

  async add(id: string, file: File): Promise<MouseMeasures> {
    const f = this.local.find(id);
    if (!f) throw new Error('The recording is gone');
    if (!f.stats)
      throw new Error(
        "The recording has no stats file: a log is matched with the run by the stats file's kill times. Pair one first (Stats file).",
      );
    const log = await file.arrayBuffer();
    const measured = await this.read(new Uint8Array(log), f.stats, file.name);
    if (!measured.run) throw new Error(`${file.name}: ${measured.error}`);
    await this.saved.save(f.file, file.name, log);
    return measured;
  }

  async forget(id: string): Promise<void> {
    const f = this.local.find(id);
    if (f) await this.saved.forget(f.file);
  }

  logger(): ResourceRef<MouseLoggerState | null | undefined> {
    return resource<MouseLoggerState | null, true>({
      params: () => true,
      loader: () => Promise.resolve(null),
    });
  }

  setLogger(): Promise<MouseLoggerState> {
    return Promise.reject(
      new Error('The browser cannot log the mouse: use the desktop app or python/mouse_log.py'),
    );
  }

  private async read(log: Uint8Array, stats: StatsCsv, file: string): Promise<MouseMeasures> {
    const request: MouseReadRequest = {
      stats_name: stats.name,
      stats_text: stats.text,
      utc_offset: logUtcOffset(log),
    };
    const outcome = JSON.parse(
      await this.core.mouseRead(log, JSON.stringify(request)),
    ) as MouseReadOutcome;
    if ('run' in outcome) return { file, run: outcome.run, error: null };
    return { file, run: null, error: 'error' in outcome ? outcome.error : 'no run to measure' };
  }
}
