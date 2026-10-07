/**
 * Browser mode's `StatsFiles`. In: KovaaK's folders the user chooses as files (kovaak-copy.ts
 * copies them into the browser) and the service in the page's /api/stats. Out: each recording's
 * stats file, and what KovaaK's folders still lack, for the stats file panel and the top bar.
 */

import { computed, inject, Service } from '@angular/core';
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
 * Browser mode's stats files: the review service pairs each recording with one of KovaaK's stats
 * files, as the review server does, from the copy of KovaaK's folders kept in this browser. The
 * user chooses those folders as files (Stats folder at the top); the page copies the new or
 * changed files in (KovaakCopy), and the list is read again.
 */
@Service()
export class BrowserStatsFiles extends ServerStatsFiles {
  /** The copy of KovaaK's folders in this browser. */
  private readonly kovaak = inject(KovaakCopy);
  /** The recordings list, read again once new stats files are in. */
  private readonly recordings = inject(BrowserRecordings);
  /** True once the browser holds a copy of KovaaK's stats folder. */
  override readonly searches = computed(() => this.kovaak.found()?.has('stats') ?? false);

  /**
   * Which of KovaaK's folders the copy lacks and how to give them, in words; null while none was
   * chosen, during a copy, or when none is missing.
   */
  override readonly missing = computed<string | null>(() => {
    const found = this.kovaak.found();
    if (!found || this.kovaak.transfer()) return null;
    const missing = FOLDER_ROLES.filter((role) => !found.has(role));
    if (!missing.length) return null;
    return (
      `Missing: ${missing.map((role) => ROLE_NAMES[role]).join(', ')}. Give them with Stats folder at the top: ` +
      String.raw`FPSAimTrainer (in steamapps\common) for the stats and your scenarios, workshop\content\824270 ` +
      "for the workshop's. The stats folder lets each run find its stats file by scenario and time; the scenario " +
      "folders give each scenario's kind, time limit and target count. The browser keeps a copy of them: choose " +
      'the folder again after new runs.'
    );
  });

  /** KovaaK's folders chosen as files: copied into this browser, then the recordings relisted. */
  override readonly chooseFolder = async (files: File[]): Promise<void> => {
    await this.kovaak.copy(files);
    this.recordings.reload();
  };
}
