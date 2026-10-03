import { ResourceRef } from '@angular/core';

/**
 * A past run of a scenario, from its stats file: when it ended (the file name's time stamp, yyyy.mm.dd-hh.mm.ss), its
 * score, its kills, and its accuracy (hits over shots). Kills and accuracy are null when the file lacks them.
 */
export interface PastRun {
  stamp: string;
  score: number;
  kills: number | null;
  accuracy: number | null;
}

/**
 * Every past run of a scenario, from KovaaK's stats files (the files with a score). Each mode provides one
 * (modes/mode.*.ts).
 */
export abstract class ScoreHistory {
  /**
   * The scenario's runs, oldest first; empty when the mode cannot reach the stats files yet. Call it where a resource
   * can be made (a field of a component or service).
   */
  abstract runs(scenario: () => string | undefined): ResourceRef<PastRun[] | undefined>;
}
