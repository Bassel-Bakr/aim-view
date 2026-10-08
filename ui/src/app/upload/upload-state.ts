/**
 * What the top bar's Upload says (`UploadState`): the note of what the last upload or folder did,
 * and what is being done. In: Upload and FolderPicks, which write it, and the RecordingSource's
 * transfer (through Library). Out: the note beside the Upload button.
 */

import { computed, inject, Service, signal } from '@angular/core';
import { formatCount, formatPercent } from '../format';
import { Library } from '../services/library';

/** What the last upload did, in words; failed for a file that could not be used. */
export interface UploadNote {
  /** The words to show. */
  text: string;
  /** Whether a file could not be used, so the note shows as an error. */
  failed: boolean;
}

/** What the top bar says is being done: the words, and the share done (null while it is not known). */
export interface BusyNote {
  /** What is being done, with how far it is. */
  text: string;
  /** The share done, 0 to 1; null while not known. */
  share: number | null;
}

/** How long a note stays, in ms. */
const NOTE_MS = 6000;

/**
 * The note Upload shows, shared with FolderPicks: the folder buttons can sit in the top bar's More menu, and what
 * they did still shows beside Upload.
 */
@Service()
export class UploadState {
  /** Where the recordings come from, for the transfer under way. */
  private readonly source = inject(Library).source;
  /** What the last upload did; null once the note has gone (after NOTE_MS). */
  readonly note = signal<UploadNote | null>(null);
  /** A folder being opened: from the click until the browser hands its files over (it lists them first). */
  readonly opening = signal<string | null>(null);
  /** What is being done, in words with how far it is (items done of how many, or a share), for the top bar. */
  readonly busy = computed<BusyNote | null>(() => {
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
  /** Hides the note once it fires. */
  private noteTimer = 0;

  /** Shows a note for NOTE_MS; `failed` shows it as an error. */
  show(text: string, failed: boolean): void {
    this.note.set({ text, failed });
    clearTimeout(this.noteTimer);
    this.noteTimer = window.setTimeout(() => this.note.set(null), NOTE_MS);
  }
}
