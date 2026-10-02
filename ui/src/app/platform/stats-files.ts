import { ResourceRef, Signal } from '@angular/core';
import { StatsChange, StatsChoice, StatsPairing } from '../api';

/**
 * What the user can do so KovaaK's stats files can be listed (open the stats folder): what the button says, what it
 * is for, and the action, run in the click. files: the folder chosen as files instead (a folder input), where the
 * browser's folder picker cannot open it.
 */
export interface StatsSetup {
  label: string;
  detail: string;
  run: () => Promise<void>;
  files: ((files: File[]) => Promise<void>) | null;
}

/**
 * A recording's stats file: the one it has, the ones to pair it with, and the user's choice. Each mode provides one
 * (modes/mode.*.ts).
 */
export abstract class StatsFiles {
  /** It can list KovaaK's stats files to pair with (it reaches the stats folder). */
  abstract readonly searches: Signal<boolean>;
  /** What the user can do so it can list them; null when nothing is needed. */
  abstract readonly setup: Signal<StatsSetup | null>;

  /**
   * The open recording's stats file and the files to pair it with: its scenario's, or with a query those of every
   * scenario whose name holds it. Call it where a resource can be made (a field of a component or service).
   */
  abstract pairing(
    id: () => string | undefined,
    query: () => string | null,
  ): ResourceRef<StatsPairing | undefined>;

  /** Pairs the recording with one of the listed files, or with none; or finds it by name and time again. */
  abstract choose(id: string, choice: StatsChoice): Promise<StatsChange>;

  /** Pairs the recording with a stats file from this computer. Rejects a file that is not one of KovaaK's. */
  abstract pairFile(id: string, file: File): Promise<StatsChange>;
}
