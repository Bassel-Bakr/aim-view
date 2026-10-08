/**
 * The top bar's folder buttons (`FolderPicks`): opening a folder of recordings (VODs folder) and
 * choosing KovaaK's stats folder, where the mode offers them. In: the RecordingSource (through
 * Library) and the StatsFiles contract. Out: the recordings and stats they read, and the note of
 * what happened (UploadState, shown beside Upload).
 */

import { Component, inject } from '@angular/core';
import { errorMessage } from '../../api';
import { FolderAction } from '../../platform/recording-source';
import { StatsFiles } from '../../platform/stats-files';
import { Library } from '../../services/library';
import { UploadState } from '../upload-state';

/**
 * The less-used ways to add: a folder of recordings, and KovaaK's stats folder. Apart from Upload so they can sit in
 * the top bar's More menu; what they did shows beside Upload.
 */
@Component({
  selector: 'app-folder-picks',
  templateUrl: './folder-picks.html',
  styleUrl: './folder-picks.scss',
})
export class FolderPicks {
  /** The mode's stats files: choosing KovaaK's stats folder. */
  protected readonly stats = inject(StatsFiles);
  /** Where the recordings come from: its folder action, and the list it fills. */
  protected readonly source = inject(Library).source;
  /** The note beside Upload, and the folder being opened. */
  protected readonly state = inject(UploadState);

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
      if (count) this.state.show(`${count} recordings in the list`, false);
    } catch (error) {
      this.state.show(`Could not open the folder: ${errorMessage(error)}`, true);
    }
  }

  /** Opens the folder input, showing that the folder is being opened until the browser hands its files over. */
  protected chooseStatsFolder(input: HTMLInputElement): void {
    this.state.opening.set('Opening the stats folder');
    input.click();
  }

  /** KovaaK's stats folder chosen as files: each recording then finds its stats file. */
  protected pickStatsFolder(
    input: HTMLInputElement,
    choose: (files: File[]) => Promise<void>,
  ): void {
    const files = [...(input.files ?? [])];
    input.value = '';
    this.state.opening.set(null);
    if (!files.length) return;
    choose(files).then(
      () => this.state.show("KovaaK's stats folder is read", false),
      (error: unknown) =>
        this.state.show(`Could not read the folder: ${errorMessage(error)}`, true),
    );
  }
}
