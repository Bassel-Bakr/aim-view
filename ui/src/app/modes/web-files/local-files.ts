import { computed, inject, Injectable, signal, WritableSignal } from '@angular/core';
import { errorMessage, Kind, LinkInfo, Recording, StatsHow } from '../../api';
import {
  AddResult,
  FolderAction,
  ItemCount,
  RecordingSource,
  Transfer,
  VideoState,
} from '../../platform/recording-source';
import {
  parseStatsCsv,
  parseTitledName,
  parseVodName,
  stampSeconds,
  statsForVideo,
  StatsCsv,
  statsSummary,
} from './stats-csv';
import { exampleRec } from './area-examples';
import { BrowserLinks } from './browser-links';
import { BrowserStore } from './browser-store';
import { KovaakFolders } from './kovaak-folders';
import { LabelMarks } from './label-marks';
import { FolderEntry, RecordingsFolder } from './recordings-folder';
import { SavedReviews } from './saved-reviews';
import { ScenarioFacts } from './scenario-facts';
import { StatsFolder } from './stats-folder';
import { isCsv, isMp4, isVideo, toMp4 } from './video-files';

const LOCAL = 'local:';
/** A recording of the recordings folder: its id is its path there, so a link to it lasts. */
const FOLDER = 'folder:';
/** Recordings whose stats files are looked up and read together, then shown at once. */
const STATS_BATCH = 50;
/** The stats files paired with the recordings folder's videos, kept in this browser by the video's id. */
const PAIRS_KEY = 'stats-pairs';

/** A recording's stats file as the browser keeps it across visits: the file (null: it has none), and how it came. */
export interface KeptStats {
  stats: StatsCsv | null;
  how: StatsHow;
}

/** The kept stats files, by recording id. */
export type KeptPairs = Record<string, KeptStats>;

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

/**
 * A local file as a row of the recordings list; the stats file gives what the video's name does not, and the
 * scenario's file its kind (kindOf, when KovaaK's scenarios are open).
 */
export function localRecording(
  f: LocalFile,
  kindOf: (scenario: string) => Kind | null = () => null,
): Recording {
  const vod = parseVodName(f.file.name);
  const titled = vod ? null : parseTitledName(f.file.name);
  const stats = f.stats && statsSummary(f.stats);
  const stamp = new Date(f.file.lastModified);
  const pad = (n: number) => String(n).padStart(2, '0');
  const scenario =
    vod?.scenario ?? stats?.scenario ?? titled?.title ?? f.file.name.replace(/\.\w+$/, '');
  return {
    id: f.id,
    scenario,
    kind: kindOf(scenario),
    score: vod?.score ?? stats?.score ?? null,
    stamp:
      vod?.stamp ??
      stats?.stamp ??
      titled?.stamp ??
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

/** Why a remembered folder's VODs are not listed. */
const goneText = (name: string) =>
  `${name} could not be found: it was moved or deleted, or its drive is not connected. `;

/** A step done item by item, with how far it is. */
function counted(label: string, count: ItemCount | null): Transfer {
  return count
    ? { label, share: count.total ? count.done / count.total : null, count }
    : { label, share: null };
}

/** A file as a recording: ready to play when it is an MP4, else waiting for its remux. */
function localFile(id: string, file: File, added: number): LocalFile {
  return {
    id,
    file,
    video: signal<VideoState>(
      isMp4(file)
        ? { state: 'ready', url: URL.createObjectURL(file), remuxed: false }
        : { state: 'remuxing', progress: 0 },
    ),
    stats: null,
    statsHow: 'missing',
    added,
    changes: {},
  };
}

/** Reads a .csv file as a stats file, or null when it is not one. */
export async function readStats(file: File): Promise<StatsCsv | null> {
  return parseStatsCsv(file.name, await file.text());
}

/**
 * The recordings opened from this computer, newest first, each played from the browser's own copy: files added, and
 * the recordings folder's videos (RecordingsFolder). Remuxes run one at a time, so two large videos are never in memory
 * at once; a folder's video is remuxed when it is first opened.
 */
@Injectable({ providedIn: 'root' })
export class LocalFiles implements RecordingSource {
  private readonly statsFolder = inject(StatsFolder);
  private readonly scenarios = inject(ScenarioFacts);
  private readonly recordingsFolder = inject(RecordingsFolder);
  private readonly kovaak = inject(KovaakFolders);
  private readonly store = inject(BrowserStore);
  private readonly saved = inject(SavedReviews);
  private readonly marks = inject(LabelMarks);
  private readonly links = inject(BrowserLinks);
  readonly linkServer = this.links.server;
  private kept: KeptPairs = {};
  readonly files = signal<LocalFile[]>([]);
  readonly recordings = computed<Recording[]>(() => {
    const facts = this.scenarios.byName();
    const kindOf = (s: string) => facts.get(s.toLowerCase())?.kind ?? null;
    const notAim = this.marks.notAim();
    return this.files().map((f) => {
      const r = { ...localRecording(f, kindOf), not_aim: notAim.has(exampleRec(f.id)) };
      return r.analysed || !this.saved.has(f.file) ? r : { ...r, analysed: true };
    });
  });
  /** The recordings folder is being read and nothing is listed yet. */
  readonly loading = computed(() => this.recordingsFolder.state().busy && !this.files().length);
  /** A remembered recordings folder that could not be found, shown above the list. */
  readonly problem = computed(() => {
    const gone = this.recordingsFolder.state().gone;
    return gone ? `${goneText(gone)}Open it again with VODs folder when it is back.` : null;
  });
  readonly addedFilesGo = 'They stay in this browser.';
  private readonly remuxing = signal<Transfer | null>(null);
  private readonly finding = signal<Transfer | null>(null);
  /** What is being done for the list: a remux, reading a folder or the scenarios, or finding the stats files. */
  readonly transfer = computed<Transfer | null>(() => {
    const folder = this.recordingsFolder.state();
    if (folder.busy) return counted(`Reading ${folder.name}`, folder.count);
    if (this.kovaak.state().busy)
      return counted("Reading KovaaK's folders", this.scenarios.progress());
    const keeping = this.kovaak.keeping();
    return (
      this.remuxing() ??
      this.finding() ??
      (keeping && counted('Keeping the stats files in this browser', keeping))
    );
  });
  private count = 0;
  private remuxes: Promise<void> = Promise.resolve();
  private readonly queued = new Set<string>();

  readonly folder = computed<FolderAction>(() => {
    const s = this.recordingsFolder.state();
    const open = (entries: () => Promise<FolderEntry[] | null>) => async () => {
      const found = await entries();
      if (found) await this.addFolder(found);
    };
    return {
      label: 'VODs folder',
      detail:
        (s.refused ? `${s.refused}. ` : '') +
        (s.gone ? goneText(s.gone) : '') +
        (s.ask
          ? `Let the browser read ${s.name} again.`
          : s.name
            ? `The VODs of ${s.name} are listed. Open another folder of VODs.`
            : 'Open a folder of VODs (KovOBS keeps one folder per scenario). They stay in this browser, ' +
              'which remembers the folder.'),
      busy: s.busy,
      run: open(() => (s.ask ? this.recordingsFolder.allow() : this.recordingsFolder.open())),
      files:
        this.recordingsFolder.picker && !s.refused
          ? null
          : (files) => open(async () => this.recordingsFolder.chosen(files))(),
    };
  });

  constructor() {
    void this.restore();
  }

  /** The stats files kept from a visit before, then the recordings folder (its videos paired with them). */
  private async restore(): Promise<void> {
    this.kept = (await this.store.get<KeptPairs>(PAIRS_KEY).catch(() => undefined)) ?? {};
    await this.kovaak.restored;
    const found = await this.recordingsFolder.restore();
    if (found) await this.addFolder(found);
  }

  video(id: string): VideoState | null {
    const f = this.find(id);
    if (!f) return null;
    // a folder's video is remuxed when it is first opened, not when the folder is listed
    if (f.video().state === 'remuxing' && !this.queued.has(f.id)) this.queue(f);
    return f.video();
  }

  readonly clearable = true;

  /** Empties the list and frees the browser's copies, and forgets the recordings folder. */
  async clear(): Promise<void> {
    for (const f of this.files()) {
      const v = f.video();
      if (v.state === 'ready' || v.state === 'failed') URL.revokeObjectURL(v.url);
    }
    this.files.set([]);
    this.queued.clear();
    this.kept = {};
    await this.store.remove(PAIRS_KEY).catch(() => undefined);
    await this.recordingsFolder.forget();
  }

  /** A folder's recording opens again after the page loads (once the folder is read again). */
  lasting(id: string): boolean {
    return id.startsWith(FOLDER);
  }

  /** Opens the videos among files, each with its stats file among the .csv files when one matches it. */
  async add(files: readonly File[]): Promise<AddResult> {
    const videos = files.filter(isVideo);
    const csvs = files.filter(isCsv);
    const read = await Promise.all(csvs.map(readStats));
    const stats = read.filter((s): s is StatsCsv => s !== null);
    const added = videos.map((file) => {
      const f = localFile(`${LOCAL}${++this.count}/${file.name}`, file, Date.now());
      f.stats = statsForVideo(file.name, stats, videos.length === 1);
      f.statsHow = f.stats ? 'upload' : 'missing';
      return f;
    });
    this.files.update((list) => [...[...added].reverse(), ...list]);
    for (const f of added) if (f.video().state === 'remuxing') this.queue(f);
    await Promise.all(added.filter((f) => !f.stats).map((f) => this.findStats(f.id)));
    return {
      ids: added.map((f) => f.id),
      notStats: csvs.filter((_, i) => read[i] === null).map((f) => f.name),
    };
  }

  linkInfo(url: string): Promise<LinkInfo> {
    return this.links.info(url);
  }

  /**
   * Adds a link's video as a file added from this computer: listed at once, with an empty file while it downloads
   * (BrowserLinks), which the video takes the place of when it is all here.
   */
  async addLink(url: string, format: string | null): Promise<string> {
    const video = signal<VideoState>({ state: 'downloading', label: '', done: 0, total: 0 });
    const started = await this.links.start(url, format, (label, done, total) =>
      video.set({ state: 'downloading', label, done, total }),
    );
    const f: LocalFile = {
      id: `${LOCAL}${++this.count}/${started.name}`,
      file: new File([], started.name),
      video,
      stats: null,
      statsHow: 'missing',
      added: Date.now(),
      changes: {},
    };
    this.files.update((list) => [f, ...list]);
    started.file.then(
      (file) => this.fill(f.id, file),
      (e: unknown) => video.set({ state: 'not-downloaded', error: errorMessage(e) }),
    );
    return f.id;
  }

  /** A link's video, all here, in place of its empty file: then as a file added from this computer. */
  private async fill(id: string, file: File): Promise<void> {
    const f = this.find(id);
    if (!f) return;
    const filled: LocalFile = { ...localFile(id, file, f.added), changes: f.changes };
    this.update(id, () => filled);
    if (filled.video().state === 'remuxing') this.queue(filled);
    await this.findStats(id);
  }

  patch(id: string, change: Partial<Recording>): void {
    this.update(id, (f) => ({ ...f, changes: { ...f.changes, ...change } }));
  }

  /** Pairs a recording with a stats file; false when the file is not one of KovaaK's stats files. */
  async pair(id: string, file: File, how: StatsHow = 'upload'): Promise<boolean> {
    const stats = await readStats(file);
    if (stats) this.setStats(new Map([[id, { stats, how }]]));
    return stats !== null;
  }

  /** The user says the recording has no stats file. */
  unpair(id: string): void {
    this.setStats(new Map([[id, { stats: null, how: 'none' }]]));
  }

  /**
   * Finds the recording's stats file in KovaaK's stats folder by its scenario and time (within five seconds), as the
   * review server does; with the folder not open, or no such file, it has none. Returns whether it found one.
   */
  async findStats(id: string): Promise<boolean> {
    const f = this.find(id);
    if (!f) return false;
    const r = localRecording(f);
    const entry = this.statsFolder.ready()
      ? this.statsFolder.find(r.scenario, stampSeconds(r.stamp))
      : null;
    if (!entry) {
      this.setStats(new Map([[id, { stats: null, how: 'missing' }]]));
      return false;
    }
    return this.pair(id, await this.statsFolder.read(entry.name), 'found');
  }

  /**
   * The recordings folder's videos, newest first, in place of those listed before; each finds its stats file when
   * KovaaK's stats folder is open.
   */
  async addFolder(entries: readonly FolderEntry[]): Promise<void> {
    const added = entries
      .map((e) => {
        const f = localFile(`${FOLDER}${e.path}`, e.file, e.file.lastModified);
        const kept = this.kept[f.id];
        return kept ? { ...f, stats: kept.stats, statsHow: kept.how } : f;
      })
      .sort((a, b) => b.added - a.added);
    for (const f of this.files()) {
      const v = f.video();
      if (f.id.startsWith(FOLDER) && (v.state === 'ready' || v.state === 'failed'))
        URL.revokeObjectURL(v.url);
    }
    this.files.update((list) => [...list.filter((f) => !f.id.startsWith(FOLDER)), ...added]);
    await this.findAllStats();
  }

  /**
   * Finds the stats file of every recording that has none and was not told it has none (the folder just opened), a
   * batch at a time: the list changes once a batch.
   */
  async findAllStats(): Promise<void> {
    if (!this.statsFolder.ready()) return;
    const open = this.files().filter((f) => f.statsHow === 'missing');
    const label = "Finding each recording's stats file";
    try {
      for (let k = 0; k < open.length; k += STATS_BATCH) {
        this.finding.set(counted(label, { done: k, total: open.length }));
        await this.findBatch(open.slice(k, k + STATS_BATCH));
      }
    } finally {
      this.finding.set(null);
    }
  }

  /** A batch of recordings' stats files, found by name and time, read, and shown at once. */
  private async findBatch(batch: readonly LocalFile[]): Promise<void> {
    const found = new Map<string, KeptStats>();
    await Promise.all(
      batch.map(async (f) => {
        const r = localRecording(f);
        const entry = this.statsFolder.find(r.scenario, stampSeconds(r.stamp));
        const stats = entry && (await readStats(await this.statsFolder.read(entry.name)));
        if (stats) found.set(f.id, { stats, how: 'found' });
      }),
    );
    if (found.size) this.setStats(found);
  }

  /**
   * Sets recordings' stats files, and keeps those of the recordings folder's videos for the next visit (the stats
   * folder, chosen as files, is not kept).
   */
  private setStats(changes: ReadonlyMap<string, KeptStats>): void {
    this.files.update((list) =>
      list.map((f) => {
        const c = changes.get(f.id);
        return c ? { ...f, stats: c.stats, statsHow: c.how } : f;
      }),
    );
    let kept = false;
    for (const [id, c] of changes) {
      if (!this.lasting(id)) continue;
      if (c.how === 'missing') delete this.kept[id];
      else this.kept[id] = c;
      kept = true;
    }
    if (kept) void this.store.set(PAIRS_KEY, this.kept).catch(() => undefined);
  }

  find(id: string | null): LocalFile | null {
    return this.files().find((f) => f.id === id) ?? null;
  }

  /** Queues a file's remux into MP4 behind the others. */
  private queue(f: LocalFile): void {
    this.queued.add(f.id);
    this.remuxes = this.remuxes.then(() => this.remux(f));
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
        this.remuxing.set({ label, share: progress });
      });
      f.video.set({ state: 'ready', url: URL.createObjectURL(mp4), remuxed: true });
    } catch (e) {
      f.video.set({ state: 'failed', url: URL.createObjectURL(f.file), error: errorMessage(e) });
    } finally {
      this.remuxing.set(null);
    }
  }

  private update(id: string, change: (f: LocalFile) => LocalFile): void {
    this.files.update((list) => list.map((f) => (f.id === id ? change(f) : f)));
  }
}
