import { Component, computed, DestroyRef, DOCUMENT, inject, signal } from '@angular/core';
import { errorMessage } from '../api';
import { formatPercent } from '../format';
import { StatsFiles } from '../platform/stats-files';
import { Library } from '../services/library';
import { button } from '@themes/controls.styles';
import { uploadStyles } from '@themes/upload.styles';
import { slotClasses } from '@themes/slot-classes';

/** What the last upload did, in words; failed for a file that could not be used. */
export interface UploadNote {
  text: string;
  failed: boolean;
}

const NOTE_MS = 6000;

/**
 * Upload: recordings from this computer, by the button or dropped anywhere on the page, with their stats .csv files.
 * Where they go is the mode's (RecordingSource). A stats file dropped alone pairs with the open recording.
 */
@Component({
  selector: 'app-upload',
  templateUrl: './upload.html',
  host: {
    '(document:dragenter)': 'showDropZone($event)',
    '(document:dragleave)': 'hideDropZone()',
    '(document:drop)': 'dropFiles($event)',
  },
})
export class Upload {
  private readonly library = inject(Library);
  private readonly stats = inject(StatsFiles);
  protected readonly source = this.library.source;
  /** What is being prepared or sent, with the share done. */
  protected readonly transfer = computed(() => {
    const t = this.source.transfer();
    return t && `${t.label}${t.share === null ? '…' : `: ${formatPercent(t.share)}`}`;
  });
  protected readonly ui = slotClasses(uploadStyles());
  protected readonly button = button();
  protected readonly dragging = signal(false);
  protected readonly note = signal<UploadNote | null>(null);
  private depth = 0;
  private noteTimer = 0;

  constructor() {
    // dragover fires many times a second: a plain listener, so it never runs change detection
    const document = inject(DOCUMENT);
    const allowDrop = (e: DragEvent) => e.preventDefault();
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

  protected showDropZone(e: DragEvent): void {
    if (!e.dataTransfer?.types.includes('Files')) return;
    this.depth++;
    this.dragging.set(true);
  }

  protected hideDropZone(): void {
    if (--this.depth > 0) return;
    this.depth = 0;
    this.dragging.set(false);
  }

  protected dropFiles(e: DragEvent): void {
    e.preventDefault();
    this.depth = 0;
    this.dragging.set(false);
    const files = [...(e.dataTransfer?.files ?? [])];
    if (files.length) void this.open(files);
  }

  /** Adds the videos among files and opens the first; a stats file alone pairs with the open recording. */
  private async open(files: File[]): Promise<void> {
    const id = this.library.selectedId();
    const videos = files.filter((f) => /\.(mp4|mkv|mov|webm)$/i.test(f.name));
    const csvs = files.filter((f) => /\.csv$/i.test(f.name));
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
    } catch (e) {
      this.show(`Could not add the files: ${errorMessage(e)}`, true);
    }
  }

  private show(text: string, failed: boolean): void {
    this.note.set({ text, failed });
    clearTimeout(this.noteTimer);
    this.noteTimer = window.setTimeout(() => this.note.set(null), NOTE_MS);
  }
}
