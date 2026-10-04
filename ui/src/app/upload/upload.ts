import { Component, computed, DestroyRef, DOCUMENT, inject, signal } from '@angular/core';
import { Button } from '../controls/button';
import { errorMessage } from '../api';
import { formatCount, formatPercent } from '../format';
import { FolderAction } from '../platform/recording-source';
import { StatsFiles } from '../platform/stats-files';
import { Library } from '../services/library';
import { LinkForm } from './link-form/link-form';

/** What the last upload did, in words; failed for a file that could not be used. */
export interface UploadNote {
  text: string;
  failed: boolean;
}

const NOTE_MS = 6000;

/** What the top bar says is being done: the words, and the share done (null while it is not known). */
export interface BusyNote {
  text: string;
  share: number | null;
}

/**
 * Upload: recordings from this computer, by the button or dropped anywhere on the page, with their stats .csv files,
 * or from a link (LinkForm).
 * Where they go is the mode's (RecordingSource). A stats file dropped alone pairs with the open recording.
 */
@Component({
  imports: [Button, LinkForm],
  selector: 'app-upload',
  templateUrl: './upload.html',
  styleUrl: './upload.scss',
  host: {
    '(document:dragenter)': 'showDropZone($event)',
    '(document:dragleave)': 'hideDropZone()',
    '(document:drop)': 'dropFiles($event)',
  },
})
export class Upload {
  private readonly library = inject(Library);
  protected readonly stats = inject(StatsFiles);
  protected readonly source = this.library.source;
  /** What is being done, in words with how far it is (items done of how many, or a share), for the top bar. */
  protected readonly busy = computed<BusyNote | null>(() => {
    const transfer = this.source.transfer();
    if (transfer) {
      const far = transfer.count
        ? `: ${formatCount(transfer.count.done)} of ${formatCount(transfer.count.total)}`
        : transfer.share === null
          ? '…'
          : `: ${formatPercent(transfer.share)}`;
      return { text: `${transfer.label}${far}`, share: transfer.share };
    }
    const opening = this.opening();
    return opening ? { text: `${opening}…`, share: null } : null;
  });
  protected readonly dragging = signal(false);
  protected readonly note = signal<UploadNote | null>(null);
  /** A folder being opened: from the click until the browser hands its files over (it lists them first). */
  protected readonly opening = signal<string | null>(null);
  private depth = 0;
  private noteTimer = 0;

  constructor() {
    // dragover fires many times a second: a plain listener, so it never runs change detection
    const document = inject(DOCUMENT);
    const allowDrop = (event: DragEvent) => event.preventDefault();
    document.addEventListener('dragover', allowDrop);
    inject(DestroyRef).onDestroy(() => {
      document.removeEventListener('dragover', allowDrop);
      clearTimeout(this.noteTimer);
    });
  }

  protected pickFiles(input: HTMLInputElement): void {
    void this.open([...(input.files ?? [])]);
    input.value = '';
  }

  /** Opens a folder of recordings (in the click), and says how many it holds. */
  protected openFolder(action: FolderAction): void {
    void this.listFolder(() => action.run());
  }

  /** A folder of recordings chosen as files, where the browser cannot open it. */
  protected pickFolder(input: HTMLInputElement, action: FolderAction): void {
    const files = [...(input.files ?? [])];
    input.value = '';
    const read = action.files;
    if (files.length && read) void this.listFolder(() => read(files));
  }

  private async listFolder(step: () => Promise<void>): Promise<void> {
    try {
      await step();
      const count = this.source.recordings().length;
      if (count) this.show(`${count} recordings in the list`, false);
    } catch (error) {
      this.show(`Could not open the folder: ${errorMessage(error)}`, true);
    }
  }

  /** Opens the folder input, showing that the folder is being opened until the browser hands its files over. */
  protected chooseStatsFolder(input: HTMLInputElement): void {
    this.opening.set('Opening the stats folder');
    input.click();
  }

  /** KovaaK's stats folder chosen as files: each recording then finds its stats file. */
  protected pickStatsFolder(
    input: HTMLInputElement,
    choose: (files: File[]) => Promise<void>,
  ): void {
    const files = [...(input.files ?? [])];
    input.value = '';
    this.opening.set(null);
    if (!files.length) return;
    choose(files).then(
      () => this.show("KovaaK's stats folder is read", false),
      (error: unknown) => this.show(`Could not read the folder: ${errorMessage(error)}`, true),
    );
  }

  protected showDropZone(event: DragEvent): void {
    if (!event.dataTransfer?.types.includes('Files')) return;
    this.depth++;
    this.dragging.set(true);
  }

  protected hideDropZone(): void {
    if (--this.depth > 0) return;
    this.depth = 0;
    this.dragging.set(false);
  }

  protected dropFiles(event: DragEvent): void {
    event.preventDefault();
    this.depth = 0;
    this.dragging.set(false);
    const files = [...(event.dataTransfer?.files ?? [])];
    if (files.length) void this.open(files);
  }

  /** Adds the videos among files and opens the first; a stats file alone pairs with the open recording. */
  private async open(files: File[]): Promise<void> {
    const id = this.library.selectedId();
    const videos = files.filter((file) => /\.(mp4|mkv|mov|webm)$/i.test(file.name));
    const csvs = files.filter((file) => /\.csv$/i.test(file.name));
    try {
      if (!videos.length && csvs.length === 1 && id !== null) {
        const change = await this.stats.pairFile(id, csvs[0]);
        this.library.source.patch(id, { stats: change.stats });
        this.show(`Paired with ${csvs[0].name}`, false);
        return;
      }
      if (!videos.length) {
        this.show(
          'Pick a video (.mp4, .mkv, .mov or .webm), and its stats .csv if you have it',
          true,
        );
        return;
      }
      const added = await this.source.add(files);
      if (added.ids.length) this.library.selectedId.set(added.ids[0]);
      const bad = added.notStats.length ? ` · not a stats file: ${added.notStats.join(', ')}` : '';
      const what = videos.length === 1 ? videos[0].name : `${videos.length} recordings`;
      this.show(`Added ${what}${bad}`, !!bad);
    } catch (error) {
      this.show(`Could not add the files: ${errorMessage(error)}`, true);
    }
  }

  private show(text: string, failed: boolean): void {
    this.note.set({ text, failed });
    clearTimeout(this.noteTimer);
    this.noteTimer = window.setTimeout(() => this.note.set(null), NOTE_MS);
  }
}
