/**
 * The top bar's Upload (`Upload`): adding recordings and stats files from this computer (the
 * button, or a drop anywhere on the page) or from a link. In: the RecordingSource (through Library),
 * the StatsFiles contract and UploadState. Out: the recordings they add, and the note of what
 * happened (its own and FolderPicks').
 */

import { Component, DestroyRef, DOCUMENT, inject, input, signal } from '@angular/core';
import { Button } from '../controls/button';
import { errorMessage } from '../api';
import { StatsFiles } from '../platform/stats-files';
import { Library } from '../services/library';
import { LinkForm } from './link-form/link-form';
import { UploadState } from './upload-state';

/**
 * Upload: recordings from this computer, by the button or dropped anywhere on the page, with their stats .csv files,
 * or from a link (LinkForm), and the note of what the last upload or folder did (UploadState).
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
  /** Whether From a link shows beside the button (on a phone it is in the top bar's More menu instead). */
  readonly withLink = input(true);
  /** The recordings and the open one. */
  private readonly library = inject(Library);
  /** The mode's stats files: pairing a dropped .csv with the open recording. */
  private readonly stats = inject(StatsFiles);
  /** Where the recordings come from and where added files go. */
  protected readonly source = this.library.source;
  /** The note beside the button, shared with FolderPicks. */
  protected readonly state = inject(UploadState);
  /** Whether files are being dragged over the page, so the drop zone shows. */
  protected readonly dragging = signal(false);
  /** How many elements the drag has entered and not left: the drop zone hides at 0. */
  private depth = 0;

  /** Lets the page take a drop (dragover's default refuses it), until the component goes. */
  constructor() {
    // dragover fires many times a second: a plain listener, so it never runs change detection
    const document = inject(DOCUMENT);
    const allowDrop = (event: DragEvent) => event.preventDefault();
    document.addEventListener('dragover', allowDrop);
    inject(DestroyRef).onDestroy(() => document.removeEventListener('dragover', allowDrop));
  }

  /** Adds the files picked with the Upload button. */
  protected pickFiles(input: HTMLInputElement): void {
    void this.open([...(input.files ?? [])]);
    input.value = '';
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
        this.state.show(`Paired with ${csvs[0].name}`, false);
        return;
      }
      if (!videos.length) {
        this.state.show(
          'Pick a video (.mp4, .mkv, .mov or .webm), and its stats .csv if you have it',
          true,
        );
        return;
      }
      const added = await this.source.add(files);
      if (added.ids.length) this.library.selectedId.set(added.ids[0]);
      const bad = added.notStats.length ? ` · not a stats file: ${added.notStats.join(', ')}` : '';
      const what = videos.length === 1 ? videos[0].name : `${videos.length} recordings`;
      this.state.show(`Added ${what}${bad}`, !!bad);
    } catch (error) {
      this.state.show(`Could not add the files: ${errorMessage(error)}`, true);
    }
  }
}
