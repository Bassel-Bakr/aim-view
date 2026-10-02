import { Component, DestroyRef, DOCUMENT, inject, signal } from '@angular/core';
import { Library } from '../services/library';
import { isLocal, LocalFiles } from '../services/local-files';
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
 * They stay in this browser. A stats file dropped alone pairs with the open recording, when it is one of these.
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
  private readonly local = inject(LocalFiles);
  private readonly library = inject(Library);
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

  /** Opens the videos among files and shows the first; a stats file alone pairs with the open recording. */
  private async open(files: File[]): Promise<void> {
    const id = this.library.selectedId();
    const videos = files.filter((f) => /\.(mp4|mkv|mov|webm)$/i.test(f.name));
    const csvs = files.filter((f) => /\.csv$/i.test(f.name));
    if (!videos.length && csvs.length === 1 && id && isLocal(id)) {
      const ok = await this.local.pair(id, csvs[0]);
      this.show(ok ? `Paired with ${csvs[0].name}` : `${csvs[0].name} is not a stats file`, !ok);
      return;
    }
    if (!videos.length) {
      this.show(
        'Pick a video (.mp4, .mkv, .mov or .webm), and its stats .csv if you have it',
        true,
      );
      return;
    }
    const added = await this.local.add(files);
    this.library.selectedId.set(added.ids[0]);
    const bad = added.notStats.length ? ` · not a stats file: ${added.notStats.join(', ')}` : '';
    this.show(
      `Opened ${videos.length === 1 ? videos[0].name : `${videos.length} recordings`}${bad}`,
      !!bad,
    );
  }

  private show(text: string, failed: boolean): void {
    this.note.set({ text, failed });
    clearTimeout(this.noteTimer);
    this.noteTimer = window.setTimeout(() => this.note.set(null), NOTE_MS);
  }
}
