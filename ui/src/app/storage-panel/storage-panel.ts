/**
 * The top bar's Storage button and its dialog (`StoragePanel`): what the app keeps, each part's
 * size, and removing a part the user can do without, after a second click; then sharing
 * recordings (share-section/). In: the StoredData contract. Out: the dialog, and the removals it
 * asks for.
 */

import { Component, computed, ElementRef, inject, signal, viewChild } from '@angular/core';
import { errorMessage } from '../api';
import { DataCell } from '../data-table/data-cell';
import { DataColumn } from '../data-table/data-column';
import { DataTable } from '../data-table/data-table';
import { formatBytes } from '../format';
import { KeptData, KeptPart, StoredData } from '../platform/stored-data';
import { ShareSection } from './share-section/share-section';

/** A part as the table shows it: its name, what it holds, its size, and whether it can go. */
export interface StorageRow {
  /** The part's id. */
  id: string;
  /** What the part is, in words. */
  name: string;
  /** What it holds, or why it stays. */
  detail: string;
  /** Its size in bytes. */
  bytes: number;
  /** Whether it can be removed here. */
  removable: boolean;
  /** What removing it does, said before the second click. */
  removes: string;
}

/** A count with its noun: "1 recording", "3 recordings". */
function counted(count: number | undefined, noun: string): string {
  const many = count ?? 0;
  return `${many} ${noun}${many === 1 ? '' : 's'}`;
}

/** A part as the table shows it. */
export function storageRow(part: KeptPart): StorageRow {
  const row = (name: string, detail: string, removes = ''): StorageRow => ({
    id: part.id,
    name,
    detail,
    bytes: part.bytes,
    removable: part.removable,
    removes,
  });
  switch (part.kind) {
    case 'reviews': {
      const name = part.model ? `Reviews by ${part.model}` : 'Older reviews';
      const offered = part.listed || !part.model ? '' : '; this model is no longer offered';
      return row(
        name,
        `${counted(part.recordings, 'recording')}${offered}`,
        'Their recordings need a new review to show a report.',
      );
    }
    case 'marks':
      return row('Your marks and areas', 'Made by you: run windows, stats picks, areas, cut-offs');
    case 'cutoff':
      return row('Cut-off labels', 'Made by you: download them as cutoff.zip from a cut-off');
    case 'kovaak':
      return row(
        "KovaaK's runs and scenarios",
        `${counted(part.stats, 'stats file')}, ${counted(part.scenarios, 'scenario')}`,
        "Choose KovaaK's folders again to pair runs with their stats files.",
      );
    case 'uploads':
      return row(
        'Videos you added',
        counted(part.files, 'file'),
        'The videos you added are removed from here; your own copies stay.',
      );
    case 'mouse':
      return row('Mouse logs', `Made by you: ${counted(part.files, 'log')}`);
    case 'old_files':
      return row(
        'Files from before the database',
        'Already copied into the database',
        'The database keeps everything they held.',
      );
    case 'ffmpeg':
      return row(
        'ffmpeg',
        'Downloaded again when a review needs it',
        'It downloads again when needed.',
      );
  }
}

/** Each row's id in the table. */
function rowId(row: StorageRow): string {
  return row.id;
}

/** What the app keeps, in the top bar; it opens the parts and their sizes, where one can be removed. */
@Component({
  imports: [DataTable, DataCell, ShareSection],
  selector: 'app-storage-panel',
  templateUrl: './storage-panel.html',
  styleUrl: './storage-panel.scss',
})
export class StoragePanel {
  /** What is kept, and removing a part. */
  private readonly stored = inject(StoredData);
  /** What is kept, read when the panel is made and again on each opening. */
  private readonly read = this.stored.kept();
  /** What a removal answered, newer than the last reading; null until one. */
  private readonly removed = signal<KeptData | null>(null);
  /** The dialog. */
  private readonly dialog = viewChild.required<ElementRef<HTMLDialogElement>>('dialog');
  /** What is kept now. */
  protected readonly data = computed(
    () => this.removed() ?? (this.read.hasValue() ? this.read.value() : undefined),
  );
  /** How much is kept, and how much of it the database holds. */
  protected readonly intro = computed(() => {
    const data = this.data();
    if (!data) return '';
    const inDatabase =
      data.database === null ? '' : `, ${formatBytes(data.database)} of it in its database`;
    return `Aim View keeps ${formatBytes(data.total)} here${inDatabase}.`;
  });
  /** The parts, largest first. */
  protected readonly rows = computed(() =>
    (this.data()?.parts ?? []).map(storageRow).sort((a, b) => b.bytes - a.bytes),
  );
  /** The part whose Remove was clicked once, waiting for the second click; null: none. */
  protected readonly confirming = signal<string | null>(null);
  /** The part being removed; null: none. */
  protected readonly removing = signal<string | null>(null);
  /** Why the last removal failed; null: it did not. */
  protected readonly failed = signal<string | null>(null);
  /** The table's columns. */
  protected readonly columns: DataColumn<StorageRow>[] = [
    { id: 'name', header: 'What', text: (row) => row.name, rowHeader: true },
    { id: 'detail', header: 'Holds', text: (row) => row.detail, prose: true },
    {
      id: 'size',
      header: 'Size',
      text: (row) => formatBytes(row.bytes),
      sortBy: (row) => row.bytes,
      align: 'end',
    },
    { id: 'remove', header: '', text: () => '', sortable: false },
  ];
  /** Each row's id. */
  protected readonly rowId = rowId;

  /** Opens the dialog with what is kept read again. */
  protected open(): void {
    this.removed.set(null);
    this.confirming.set(null);
    this.failed.set(null);
    this.read.reload();
    this.dialog().nativeElement.showModal();
  }

  /** Closes the dialog. */
  protected close(): void {
    this.dialog().nativeElement.close();
  }

  /** Asks for the second click before removing the part. */
  protected ask(id: string): void {
    this.failed.set(null);
    this.confirming.set(id);
  }

  /** Keeps the part: the second click is no longer awaited. */
  protected keep(): void {
    this.confirming.set(null);
  }

  /** Removes the part, then shows what is kept now (or why it could not be removed). */
  protected async remove(id: string): Promise<void> {
    this.confirming.set(null);
    this.removing.set(id);
    try {
      this.removed.set(await this.stored.remove(id));
    } catch (error) {
      this.failed.set(errorMessage(error));
    } finally {
      this.removing.set(null);
    }
  }
}
