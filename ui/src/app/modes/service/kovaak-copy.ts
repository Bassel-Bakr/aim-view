/**
 * KovaaK's folders in browser mode. In: the files of a folder the user chooses (FPSAimTrainer, the
 * workshop's 824270, or each folder). Out: the stats and scenario files shown to the service at
 * /kovaak for this visit and sent to it once (it keeps what it needs of each), and which folders it
 * keeps files of.
 */

import { HttpClient } from '@angular/common/http';
import { inject, Service, signal } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { Transfer } from '../../platform/recording-source';
import { freshFiles, KovaakKept, sendBatches } from './kovaak-batch';
import { MountedFiles } from './mounted-files';
import { ChosenFile } from './service-messages';

/**
 * The folders of KovaaK's the review reads: its stats files, the user's scenarios, the workshop's
 * scenarios.
 */
export type FolderRole = 'stats' | 'scenarios' | 'workshop';

/** Every folder role, in the order the page names them. */
export const FOLDER_ROLES: readonly FolderRole[] = ['stats', 'scenarios', 'workshop'];

/**
 * Where each of KovaaK's files chosen as a folder goes in /kovaak, by what its path makes it: a
 * .csv in a folder named stats is a stats file (stats/), a .sce in a folder named Scenarios one of
 * the user's scenarios (scenarios/), and one in an item's folder inside 824270 the workshop's
 * (workshop/<item>/). So the user can choose steamapps, FPSAimTrainer, or each folder. Other files
 * are left out.
 */
function kovaakFiles(files: readonly File[]): ChosenFile[] {
  const out: ChosenFile[] = [];
  for (const file of files) {
    const parts = (file.webkitRelativePath || file.name).split('/');
    const folderAbove = (levelsUp: number) =>
      (parts[parts.length - 1 - levelsUp] ?? '').toLowerCase();
    if (/\.csv$/i.test(file.name) && folderAbove(1) === 'stats')
      out.push({ path: `stats/${file.name}`, file });
    else if (/\.sce$/i.test(file.name) && folderAbove(1) === 'scenarios')
      out.push({ path: `scenarios/${file.name}`, file });
    else if (/\.sce$/i.test(file.name) && folderAbove(2) === '824270')
      out.push({ path: `workshop/${parts[parts.length - 2]}/${file.name}`, file });
  }
  return out;
}

/**
 * KovaaK's folders, chosen by the user as files (a folder input: Chrome's folder picker refuses
 * folders under Program Files, where KovaaK's is). The review service reads them where they are for
 * this visit (/kovaak shows them), and each file new or changed since it was last sent is read once
 * and sent to the service (kovaak-batch.ts), which keeps each stats file's run and each scenario's
 * facts in its database, not the files: each run finds its stats file, each scenario its kind, time
 * limit and target count, on later visits too. A stats file's whole text is kept only once a
 * recording uses it.
 */
@Service()
export class KovaakCopy {
  /** Shows the chosen files to the service for this visit. */
  private readonly files = inject(MountedFiles);
  /** Asks the service what it keeps and sends it the files. */
  private readonly http = inject(HttpClient);
  /** The folders the service keeps files of; null until it is asked. */
  readonly found = signal<ReadonlySet<FolderRole> | null>(null);
  /** The sending under way, for the top bar. */
  readonly transfer = signal<Transfer | null>(null);

  /** Asks at once which folders the service keeps files of. */
  constructor() {
    void this.look();
  }

  /** What the service keeps of KovaaK's files; nothing when it cannot be asked. */
  private async kept(): Promise<KovaakKept> {
    const asked = this.http.get<KovaakKept>('/api/kovaak_files');
    return firstValueFrom(asked).catch((): KovaakKept => ({ stats: [], scenarios: [] }));
  }

  /** Sets `found`: the folders the service keeps any file of. */
  private async look(): Promise<void> {
    const kept = await this.kept();
    const has: Record<FolderRole, boolean> = {
      stats: kept.stats.length > 0,
      scenarios: kept.scenarios.some(([path]) => path.startsWith('scenarios/')),
      workshop: kept.scenarios.some(([path]) => path.startsWith('workshop/')),
    };
    this.found.set(new Set(FOLDER_ROLES.filter((role) => has[role])));
  }

  /**
   * Shows the folders chosen as files to the service for this visit, then sends it the files new or
   * changed since they were last sent, each read once (the top bar follows). Rejects when the files
   * hold no stats or scenario file of KovaaK's.
   */
  async copy(files: readonly File[]): Promise<void> {
    const chosen = kovaakFiles(files);
    if (!chosen.length)
      throw new Error("No stats or scenario files of KovaaK's in the folder chosen");
    await this.files.showKovaak(chosen);
    await this.send(chosen);
    await this.look();
  }

  /** Sends the files the service does not keep as they are now, in batches. */
  private async send(chosen: ChosenFile[]): Promise<void> {
    const fresh = freshFiles(chosen, await this.kept());
    const label = "Reading KovaaK's files into this browser";
    this.transfer.set({ label, share: null });
    try {
      const post = (body: Uint8Array) => firstValueFrom(this.http.post('/api/kovaak_files', body));
      await sendBatches(fresh, post, (done, total) =>
        this.transfer.set({ label, share: total ? done / total : null, count: { done, total } }),
      );
    } finally {
      this.transfer.set(null);
    }
  }
}
