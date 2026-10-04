import { HttpClient } from '@angular/common/http';
import { inject, Service, signal } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { Transfer } from '../../platform/recording-source';
import { MountedFiles } from './mounted-files';
import { ChosenFile } from './service-messages';

/** The folders of KovaaK's the review reads: its stats files, the user's scenarios, the workshop's scenarios. */
export type FolderRole = 'stats' | 'scenarios' | 'workshop';

export const FOLDER_ROLES: readonly FolderRole[] = ['stats', 'scenarios', 'workshop'];

/** Where KovaaK's files are copied (the contract's /kovaak). */
const KOVAAK = '/kovaak';

/**
 * Where each of KovaaK's files chosen as a folder goes in /kovaak, by what its path makes it: a .csv in a folder named
 * stats is a stats file (stats/), a .sce in a folder named Scenarios one of the user's scenarios (scenarios/), and one
 * in an item's folder inside 824270 the workshop's (workshop/<item>/). So the user can choose steamapps,
 * FPSAimTrainer, or each folder. Other files are left out.
 */
function kovaakFiles(files: readonly File[]): ChosenFile[] {
  const out: ChosenFile[] = [];
  for (const file of files) {
    const parts = (file.webkitRelativePath || file.name).split('/');
    const at = (k: number) => (parts[parts.length - 1 - k] ?? '').toLowerCase();
    if (/\.csv$/i.test(file.name) && at(1) === 'stats')
      out.push({ path: `stats/${file.name}`, file });
    else if (/\.sce$/i.test(file.name) && at(1) === 'scenarios')
      out.push({ path: `scenarios/${file.name}`, file });
    else if (/\.sce$/i.test(file.name) && at(2) === '824270')
      out.push({ path: `workshop/${parts[parts.length - 2]}/${file.name}`, file });
  }
  return out;
}

/**
 * KovaaK's folders, chosen by the user as files (a folder input: Chrome's folder picker refuses folders under Program
 * Files, where KovaaK's is), copied into this browser for the review service (/kovaak): only the files new or changed
 * since the last copy. The service then reads them again (POST /api/kovaak?changed=1): each run finds its stats file,
 * each scenario its kind, time limit and target count.
 */
@Service()
export class KovaakCopy {
  private readonly files = inject(MountedFiles);
  private readonly http = inject(HttpClient);
  /** The folders copied into this browser (any of their files); null until they have been looked at. */
  readonly found = signal<ReadonlySet<FolderRole> | null>(null);
  /** The copy under way, for the top bar. */
  readonly transfer = signal<Transfer | null>(null);

  constructor() {
    void this.look();
  }

  /** Which folders have files in this browser. */
  private async look(): Promise<void> {
    const has = await Promise.all(
      FOLDER_ROLES.map((role) =>
        this.files.list(`${KOVAAK}/${role}`, 1).then(
          (entries) => entries.length > 0,
          () => false,
        ),
      ),
    );
    this.found.set(new Set(FOLDER_ROLES.filter((_, k) => has[k])));
  }

  /** Copies the folders chosen as files into this browser, then has the service read them again. */
  async copy(files: readonly File[]): Promise<void> {
    const chosen = kovaakFiles(files);
    if (!chosen.length)
      throw new Error("No stats or scenario files of KovaaK's in the folder chosen");
    const label = "Copying KovaaK's files into this browser";
    this.transfer.set({ label, share: null });
    try {
      await this.files.copyIn(KOVAAK, chosen, (done, total) =>
        this.transfer.set({ label, share: total ? done / total : null, count: { done, total } }),
      );
      await firstValueFrom(this.http.post('/api/kovaak', null, { params: { changed: '1' } }));
    } finally {
      this.transfer.set(null);
    }
    await this.look();
  }
}
