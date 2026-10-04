import { computed, inject, Injectable, signal } from '@angular/core';
import { errorMessage, LinkInfo, Recording } from '../../api';
import { FolderAction, Transfer, VideoState } from '../../platform/recording-source';
import { SentVideo, ServerRecordings } from '../http/server-recordings';
import { BrowserLinks } from '../web-files/browser-links';
import { parseTitledName, parseVodName } from '../web-files/stats-csv';
import { isMp4, mp4Name, toMp4 } from '../web-files/video-files';
import { KovaakCopy } from './kovaak-copy';
import { MountedFiles, recordingPath } from './mounted-files';
import { VodsFolder } from './vods-folder';

/** What the run page shows while a link's video is copied into this browser. */
const COPYING = 'Copying the video into this browser';

/** Why a remembered folder's VODs are not listed. */
const goneText = (name: string) =>
  `${name} could not be found: it was moved or deleted, or its drive is not connected. `;

/** A time as a file-name stamp (yyyy.mm.dd-hh.mm.ss, local), as a recording named anyhow gets its time. */
function localStamp(d: Date): string {
  const two = (n: number) => String(n).padStart(2, '0');
  return (
    `${d.getFullYear()}.${two(d.getMonth() + 1)}.${two(d.getDate())}-` +
    `${two(d.getHours())}.${two(d.getMinutes())}.${two(d.getSeconds())}`
  );
}

/** A link's recording as listed while its video comes: named as the service names an upload (recordings.rs). */
function linkRow(id: string, name: string): Recording {
  const vod = parseVodName(name);
  const titled = vod ? null : parseTitledName(name);
  return {
    id,
    scenario: vod?.scenario ?? titled?.title ?? name.replace(/\.\w+$/, ''),
    kind: null,
    score: vod?.score ?? null,
    stamp: vod?.stamp ?? titled?.stamp ?? localStamp(new Date()),
    mtime: Date.now() / 1000,
    size: 0,
    stats: false,
    analysed: false,
    not_aim: false,
    uploaded: true,
  };
}

/**
 * Browser mode's recordings: the review service in the page lists them (its VODs folder, mounted from the folder the
 * user opened, and its uploads in this browser's storage), as the review server and the desktop app list theirs. The
 * page plays each video from the file itself (a non-MP4 remuxed into one in the browser when it is first opened),
 * opens the VODs folder, and downloads links itself (else through the Aim View server on this computer), then adds
 * them as uploads.
 */
@Injectable({ providedIn: 'root' })
export class BrowserRecordings extends ServerRecordings {
  private readonly files = inject(MountedFiles);
  private readonly vods = inject(VodsFolder);
  private readonly fetcher = inject(BrowserLinks);
  private readonly kovaak = inject(KovaakCopy);
  /** The videos opened in the page, by recording id: being remuxed, ready to play, or not. */
  private readonly opened = signal<ReadonlyMap<string, VideoState>>(new Map());
  /** Videos being opened, so each is read once. */
  private readonly opening = new Set<string>();
  private readonly remuxing = signal<Transfer | null>(null);
  /** Remuxes run one at a time, so two large videos are never in memory at once. */
  private remuxes: Promise<void> = Promise.resolve();
  override readonly linkServer = this.fetcher.server;
  override readonly addedFilesGo = "They are copied into this browser's storage.";
  override readonly clearable = true;
  override readonly transfer = computed<Transfer | null>(
    () => this.sending() ?? this.remuxing() ?? this.kovaak.transfer(),
  );
  override readonly problem = computed<string | null>(() => {
    const gone = this.vods.state().gone;
    if (gone) return `${goneText(gone)}Open it again with VODs folder when it is back.`;
    const e = this.list.error();
    return e
      ? `The review service in this page could not list the recordings: ${errorMessage(e)}`
      : null;
  });
  override readonly folder = computed<FolderAction>(() => {
    const s = this.vods.state();
    return {
      label: 'VODs folder',
      detail:
        (s.refused ? `${s.refused}. ` : '') +
        (s.gone ? goneText(s.gone) : '') +
        (s.ask
          ? `Let the browser read ${s.name} again.`
          : s.name
            ? `The VODs of ${s.name} are listed. Open another folder of VODs.`
            : 'Open a folder of VODs (KovOBS keeps one folder per scenario). They are read where they are, ' +
              'and the browser remembers the folder.'),
      busy: s.busy,
      run: () => this.mounted(s.ask ? this.vods.allow() : this.vods.open()),
      files:
        this.vods.picker && !s.refused ? null : (files) => this.mounted(this.vods.chosen(files)),
    };
  });

  constructor() {
    super();
    void this.mounted(this.vods.restore());
  }

  /** Lists the recordings again (KovaaK's folders were copied in: their stats files and kinds change). */
  reload(): void {
    this.list.reload();
  }

  /** Lists the recordings again once a VODs folder is mounted. */
  private async mounted(opening: Promise<boolean>): Promise<void> {
    if (await opening.catch(() => false)) this.list.reload();
  }

  /** A link's video, a video added or opened here; else it is read from the mounts, and null until it is. */
  override video(id: string): VideoState | null {
    const known = this.links().get(id)?.video ?? this.opened().get(id);
    if (known) return known;
    if (!this.opening.has(id)) {
      this.opening.add(id);
      // read after this call: what calls video() may be a computed signal, which must not write signals
      void Promise.resolve().then(() => this.open(id));
    }
    return null;
  }

  /** Reads a recording's video from the mounts: played as it is when it is an MP4, else once remuxed into one. */
  private async open(id: string): Promise<void> {
    let video: Blob;
    try {
      video = await this.files.read(recordingPath(id));
    } catch (e) {
      this.setOpened(id, {
        state: 'failed',
        url: '',
        error: `the video could not be read: ${errorMessage(e)}`,
      });
      return;
    }
    const name = id.split('/').pop() ?? id;
    const file = video instanceof File ? video : new File([video], name);
    if (isMp4(file)) {
      this.setOpened(id, { state: 'ready', url: URL.createObjectURL(file), remuxed: false });
      return;
    }
    this.setOpened(id, { state: 'remuxing', progress: 0 });
    this.remuxes = this.remuxes.then(() => this.remux(id, file));
  }

  /** Remuxes a video into MP4, showing the progress in whole percents (each change redraws what shows it). */
  private async remux(id: string, file: File): Promise<void> {
    const label = `Remuxing ${file.name} into MP4`;
    try {
      const mp4 = await toMp4(file, (share) => {
        const progress = Math.floor(100 * share) / 100;
        const now = this.opened().get(id);
        if (now?.state === 'remuxing' && now.progress === progress) return;
        this.setOpened(id, { state: 'remuxing', progress });
        this.remuxing.set({ label, share: progress });
      });
      this.setOpened(id, { state: 'ready', url: URL.createObjectURL(mp4), remuxed: true });
    } catch (e) {
      this.setOpened(id, {
        state: 'failed',
        url: URL.createObjectURL(file),
        error: errorMessage(e),
      });
    } finally {
      this.remuxing.set(null);
    }
  }

  private setOpened(id: string, video: VideoState): void {
    this.opened.update((all) => new Map(all).set(id, video));
  }

  /** A video sent is played from the page's own copy. */
  protected override async sendVideo(video: File): Promise<SentVideo> {
    const sent = await super.sendVideo(video);
    this.setOpened(sent.id, {
      state: 'ready',
      url: URL.createObjectURL(sent.video),
      remuxed: false,
    });
    return sent;
  }

  override linkInfo(url: string): Promise<LinkInfo> {
    return this.fetcher.info(url);
  }

  /**
   * Downloads a link's video in the page (else through the Aim View server on this computer), then adds it as an
   * upload. It is listed at once under the id the upload will have.
   */
  override async addLink(url: string, format: string | null): Promise<string> {
    let id = '';
    let row: Recording | null = null;
    const started = await this.fetcher.start(url, format, (label, done, total) => {
      if (row) this.setLink(id, { row, video: { state: 'downloading', label, done, total } });
    });
    id = this.freeUploadId(mp4Name(new File([], started.name)));
    row = linkRow(id, started.name);
    const listed = row;
    this.setLink(id, { row, video: { state: 'downloading', label: '', done: 0, total: 0 } });
    started.file
      .then((file) => this.addDownloaded(id, listed, file))
      .catch((e: unknown) =>
        this.setLink(id, {
          row: listed,
          video: { state: 'not-downloaded', error: errorMessage(e) },
        }),
      );
    return id;
  }

  /** A link's video, all here: sent to the service as an upload, then played from the page's own copy. */
  private async addDownloaded(id: string, row: Recording, file: File): Promise<void> {
    this.setLink(id, { row, video: { state: 'downloading', label: COPYING, done: 0, total: 0 } });
    try {
      const sent = await this.sendVideo(file);
      if (sent.id !== id) console.warn(`The link's video was added as ${sent.id}, not ${id}`);
      this.setLink(id, {
        row,
        video: { state: 'ready', url: URL.createObjectURL(sent.video), remuxed: false },
      });
    } finally {
      this.sending.set(null);
      this.list.reload();
    }
  }

  /** The id an upload of this name gets (the service's free_name: "name (2).mp4" when the name is taken). */
  private freeUploadId(name: string): string {
    const taken = new Set([...this.recordings().map((r) => r.id), ...this.links().keys()]);
    const dot = name.lastIndexOf('.');
    const [stem, ext] = dot > 0 ? [name.slice(0, dot), name.slice(dot)] : [name, ''];
    let id = `uploads/${name}`;
    for (let n = 2; taken.has(id); n++) id = `uploads/${stem} (${n})${ext}`;
    return id;
  }

  /**
   * Empties the list: forgets the VODs folder (the service lists nothing there then), and removes the videos added
   * here from this browser's storage. The files on this computer stay where they are.
   */
  override async clear(): Promise<void> {
    const uploads = this.recordings()
      .map((r) => r.id)
      .filter((id) => id.startsWith('uploads/'));
    for (const v of this.opened().values())
      if (v.state === 'ready' || v.state === 'failed') URL.revokeObjectURL(v.url);
    this.opened.set(new Map());
    this.opening.clear();
    this.links.set(new Map());
    this.list.set([]);
    await this.vods.forget();
    void Promise.all(
      uploads.map((id) => this.files.remove(recordingPath(id)).catch(() => undefined)),
    ).then(() => this.list.reload());
  }
}
