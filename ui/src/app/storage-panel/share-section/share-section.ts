/**
 * The Storage dialog's Share section (`ShareSection`): pick reviewed recordings and export them as
 * one zip (with their videos or not), or open an export someone sent. In: the recordings
 * (RecordingSource) and the Exports contract. Out: the zip saved, or the recordings added.
 */

import { Component, computed, inject, signal } from '@angular/core';
import { errorMessage, Recording } from '../../api';
import { DataCell } from '../../data-table/data-cell';
import { DataColumn } from '../../data-table/data-column';
import { DataTable } from '../../data-table/data-table';
import { formatBytes, formatStamp } from '../../format';
import { Exports } from '../../platform/exports';
import { RecordingSource } from '../../platform/recording-source';

/** A share as a whole percent. */
const PERCENT = 100;

/** Each recording's id in the table. */
function recordingId(recording: Recording): string {
  return recording.id;
}

/** The reviewed recordings to pick from, the picks, and the export and opening they start. */
@Component({
  imports: [DataTable, DataCell],
  selector: 'app-share-section',
  templateUrl: './share-section.html',
  styleUrl: './share-section.scss',
})
export class ShareSection {
  /** The recordings, and their videos. */
  private readonly source = inject(RecordingSource);
  /** Exports and opens zips. */
  private readonly exports = inject(Exports);
  /** The recordings with a review: only those are worth sending. */
  protected readonly reviewed = computed(() =>
    this.source.recordings().filter((recording) => recording.analysed),
  );
  /** The ids picked. */
  protected readonly picked = signal<ReadonlySet<string>>(new Set());
  /** Whether the videos go in the zip. */
  protected readonly videos = signal(true);
  /** What the export or the opening has done, 0 to 1; null while neither runs. */
  protected readonly share = signal<number | null>(null);
  /** What the last export or opening said. */
  protected readonly said = signal<string | null>(null);
  /** Whether the last one failed. */
  protected readonly failed = signal(false);
  /** The picked videos' size, for the switch's words. */
  protected readonly videoBytes = computed(() =>
    this.reviewed()
      .filter((recording) => this.picked().has(recording.id))
      .reduce((sum, recording) => sum + recording.size, 0),
  );
  /** The table's columns. */
  protected readonly columns: DataColumn<Recording>[] = [
    { id: 'pick', header: '', text: () => '', sortable: false },
    {
      id: 'scenario',
      header: 'Scenario',
      text: (recording) => recording.scenario,
      rowHeader: true,
    },
    {
      id: 'when',
      header: 'When',
      text: (recording) => formatStamp(recording.stamp),
      sortBy: (recording) => recording.stamp,
    },
    {
      id: 'size',
      header: 'Video',
      text: (recording) => formatBytes(recording.size),
      sortBy: (recording) => recording.size,
      align: 'end',
    },
  ];
  /** Each row's id. */
  protected readonly recordingId = recordingId;
  /** Sizes in words, for the template. */
  protected readonly bytes = formatBytes;

  /** Picks or unpicks a recording. */
  protected toggle(id: string): void {
    this.picked.update((now) => {
      const next = new Set(now);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  }

  /** The share in words. */
  protected percent(share: number): string {
    return `${Math.round(share * PERCENT)}%`;
  }

  /** Runs an export or an opening, saying how far it got and how it ended. */
  private async run(work: () => Promise<string>): Promise<void> {
    this.said.set(null);
    this.failed.set(false);
    this.share.set(0);
    try {
      this.said.set(await work());
    } catch (error) {
      // the user closed the save dialog: nothing happened, nothing to say
      if (error instanceof DOMException && error.name === 'AbortError') return;
      this.failed.set(true);
      this.said.set(errorMessage(error));
    } finally {
      this.share.set(null);
    }
  }

  /** Exports the picked recordings, in the list's order. */
  protected exportPicked(): void {
    const ids = this.reviewed()
      .map((recording) => recording.id)
      .filter((id) => this.picked().has(id));
    void this.run(async () => {
      await this.exports.export(ids, this.videos(), (share) => this.share.set(share));
      return `Exported ${ids.length} recording${ids.length === 1 ? '' : 's'}.`;
    });
  }

  /** Opens the export the user chose. */
  protected openFile(event: Event): void {
    const input = event.target as HTMLInputElement;
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    void this.run(async () => {
      const opened = await this.exports.open(file, (share) => this.share.set(share));
      const added = `Added ${opened.added.length} recording${opened.added.length === 1 ? '' : 's'}.`;
      return opened.skipped.length ? `${added} Left out: ${opened.skipped.join('; ')}` : added;
    });
  }
}
