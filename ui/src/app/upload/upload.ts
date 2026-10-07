/**
 * The top bar's Upload (`Upload`): adding recordings and stats files from this computer (the
 * button, or a drop anywhere on the page), from a link, or from a folder, and choosing KovaaK's
 * stats folder. In: the RecordingSource (through Library) and the StatsFiles contract. Out: the
 * recordings they add, and the note of what happened.
 */

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
  /** The words to show. */
  text: string;
  /** Whether a file could not be used, so the note shows as an error. */
  failed: boolean;
}

/** How long a note stays, in ms. */
const NOTE_MS = 6000;

/** What the top bar says is being done: the words, and the share done (null while it is not known). */
export interface BusyNote {
  /** What is being done, with how far it is. */
  text: string;
  /** The share done, 0 to 1; null while not known. */
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
  /** The recordings and the open one. */
  private readonly library = inject(Library);
  /** The mode's stats files: pairing a dropped .csv, and choosing KovaaK's stats folder. */
  protected readonly stats = inject(StatsFiles);
  /** Where the recordings come from and where added files go. */
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
  /** Whether files are being dragged over the page, so the drop zone shows. */
  protected readonly dragging = signal(false);
  /** What the last upload did; null once the note has gone (after NOTE_MS). */
  protected readonly note = signal<UploadNote | null>(null);
  /** A folder being opened: from the click until the browser hands its files over (it lists them first). */
  protected readonly opening = signal<string | null>(null);
  /** How many elements the drag has entered and not left: the drop zone hides at 0. */
  private depth = 0;
  /** Hides the note once it fires. */
  private noteTimer = 0;

  /** Lets the page take a drop (dragover's default refuses it), until the component goes. */
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

  /** Adds the files picked with the Upload button. */
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

  /** Runs a folder's opening, then says how many recordings the list holds, or why it failed. */
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

  /** Files dragged onto the page: the drop zone shows. */
  protected showDropZone(event: DragEvent): void {
    if (!event.dataTransfer?.types.includes('Files')) return;
    this.depth++;
    this.dragging.set(true);
  }

  /** The drag left an element: the drop zone hides once it has left the page. */
  protected hideDropZone(): void {
    if (--this.depth > 0) return;
    this.depth = 0;
    this.dragging.set(false);
  }

  /** Files dropped anywhere on the page: they are added. */
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

  /** Shows a note for NOTE_MS; `failed` shows it as an error. */
  private show(text: string, failed: boolean): void {
    this.note.set({ text, failed });
    clearTimeout(this.noteTimer);
    this.noteTimer = window.setTimeout(() => this.note.set(null), NOTE_MS);
  }
}
