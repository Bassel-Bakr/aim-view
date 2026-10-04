import { computed, inject, Injectable } from '@angular/core';
import { ServerStatsFiles } from '../http/server-stats-files';
import { BrowserRecordings } from './browser-recordings';
import { FOLDER_ROLES, KovaakCopy } from './kovaak-copy';

/** The folders as the user is told about them. */
const ROLE_NAMES = {
  stats: 'the stats files',
  scenarios: 'your scenarios',
  workshop: "the workshop's scenarios",
} as const;

/**
 * Browser mode's stats files: the review service pairs each recording with one of KovaaK's stats files, as the review
 * server does, from the copy of KovaaK's folders kept in this browser. The user chooses those folders as files (Stats
 * folder at the top); the page copies the new or changed files in (KovaakCopy), and the list is read again.
 */
@Injectable({ providedIn: 'root' })
export class BrowserStatsFiles extends ServerStatsFiles {
  private readonly kovaak = inject(KovaakCopy);
  private readonly recordings = inject(BrowserRecordings);
  override readonly searches = computed(() => this.kovaak.found()?.has('stats') ?? false);

  override readonly missing = computed<string | null>(() => {
    const found = this.kovaak.found();
    if (!found || this.kovaak.transfer()) return null;
    const missing = FOLDER_ROLES.filter((r) => !found.has(r));
    if (!missing.length) return null;
    return (
      `Missing: ${missing.map((r) => ROLE_NAMES[r]).join(', ')}. Give them with Stats folder at the top: ` +
      String.raw`FPSAimTrainer (in steamapps\common) for the stats and your scenarios, workshop\content\824270 ` +
      "for the workshop's. The stats folder lets each run find its stats file by scenario and time; the scenario " +
      "folders give each scenario's kind, time limit and target count. The browser keeps a copy of them: choose " +
      'the folder again after new runs.'
    );
  });

  /** KovaaK's folders chosen as files: copied into this browser, then the recordings listed again. */
  override readonly chooseFolder = async (files: File[]): Promise<void> => {
    await this.kovaak.copy(files);
    this.recordings.reload();
  };
}
