/**
 * The run page's stats file panel. In: the open recording's pairing from the mode's StatsFiles (the
 * file it has, how it came to it, the files to pair it with, what the file says) and the user's
 * search, pick or file. Out: the new pairing, the recording's stats flag in the library, and the
 * review measured again (Review follows the job).
 */

import { Component, computed, inject, input, linkedSignal, output, signal } from '@angular/core';
import {
  errorMessage,
  Recording,
  StatsChange,
  StatsChoice,
  StatsHow,
  StatsPairing,
} from '../../api';
import { formatCount, formatNumber, formatOffset, formatPercent, formatStamp } from '../../format';
import { StatsFiles } from '../../platform/stats-files';
import { Library } from '../../services/library';
import { Review } from '../../services/review';

/** How the recording came to its stats file, in the panel's words. */
const HOW: Record<StatsHow, string> = {
  picked: 'your pick',
  upload: 'chosen by you from this computer',
  none: 'you chose none',
  gone: 'your pick, but the file is gone',
  beside: 'uploaded with the recording',
  found: 'found by its name and time',
  missing: 'none found by its name and time',
};

/**
 * A stats file the panel offers: when it was written, how far from the recording, and its scenario
 * if another.
 */
export interface CandidateRow {
  /** The file's name. */
  name: string;
  /** When its run ended, in words. */
  when: string;
  /** How far its time is from the recording's, in words. */
  off: string;
  /** Its scenario when it is not the recording's; null when it is. */
  scenario: string | null;
  /** The recording is paired with this file. */
  inUse: boolean;
}

/** One fact from a stats file, as the panel lists it. */
export interface StatsFact {
  /** What the fact is ("Score"). */
  label: string;
  /** Its value in words; "–" when the file does not say. */
  value: string;
}

/** What the last change did; failed when it could not be made. */
export interface StatsMessage {
  /** What happened, in words. */
  text: string;
  /** The change failed. */
  failed: boolean;
}

/** A pairing answer, with the recording it is for. */
export interface PairingFor {
  /** The recording's id. */
  id: string;
  /** The answer; undefined while it loads or when it failed. */
  pairing: StatsPairing | undefined;
}

/**
 * Pairs the open recording with a stats file, through this mode's StatsFiles: a .csv from this
 * computer, none, or (where it reaches KovaaK's stats folder) one of its files, searched by
 * scenario and offered nearest the recording's time first. A reviewed recording is then measured
 * again with it.
 */
@Component({
  selector: 'app-stats-file',
  templateUrl: './stats-file.html',
  styleUrl: './stats-file.scss',
})
export class StatsFile {
  /** The open recording. */
  readonly recording = input.required<Recording>();
  /** Fires when the user closes the panel. */
  readonly closed = output();
  /** The mode's stats files: the pairing, the search and the changes. */
  protected readonly stats = inject(StatsFiles);
  /** The recordings: a change updates the stats flag; the template says where added files go. */
  protected readonly library = inject(Library);
  /** The open recording's review, which follows the measuring a change starts. */
  private readonly review = inject(Review);

  /**
   * The search text; null: the recording's own scenario. A newly opened recording starts again from
   * its own.
   */
  protected readonly query = linkedSignal<string, string | null>({
    source: () => this.recording().id,
    computation: () => null,
  });
  /** The recording's stats file and the files to pair it with, for the search. */
  protected readonly pairing = this.stats.pairing(
    () => this.recording().id,
    () => this.query(),
  );
  /**
   * The last answer for this recording, kept while a new search loads, so the panel does not
   * flicker.
   */
  protected readonly shown = linkedSignal<PairingFor, StatsPairing | null>({
    source: () => ({
      id: this.recording().id,
      pairing: this.pairing.hasValue() ? this.pairing.value() : undefined,
    }),
    computation: (now, previous) =>
      now.pairing ?? (previous?.source.id === now.id ? previous.value : null),
  });
  /** How the recording came to its stats file, in words; empty before the first answer. */
  protected readonly how = computed(() => {
    const pairing = this.shown();
    return pairing ? HOW[pairing.how] : '';
  });
  /** The user chose something, so finding it by name and time again changes something. */
  protected readonly chosen = computed(() =>
    ['picked', 'upload', 'none', 'gone'].includes(this.shown()?.how ?? ''),
  );
  /** The files to pair the recording with, as the list shows them, nearest in time first. */
  protected readonly rows = computed<CandidateRow[]>(() => {
    const pairing = this.shown();
    if (!pairing) return [];
    return pairing.candidates.map((candidate) => ({
      name: candidate.name,
      when: formatStamp(candidate.stamp),
      off: formatOffset(candidate.off),
      scenario:
        candidate.scenario.toLowerCase() === pairing.scenario.toLowerCase()
          ? null
          : candidate.scenario,
      inUse: candidate.name === pairing.file,
    }));
  });
  /** What the stats file says, when it was read where the page runs. */
  protected readonly facts = computed<StatsFact[] | null>(() => {
    const facts = this.shown()?.facts;
    if (!facts) return null;
    return [
      { label: 'Scenario', value: facts.scenario ?? '–' },
      { label: 'Score', value: facts.score === null ? '–' : formatNumber(facts.score) },
      { label: 'Kills', value: formatCount(facts.kills) },
      { label: 'Accuracy', value: formatPercent(facts.accuracy) },
      { label: 'Ended', value: facts.stamp ? formatStamp(facts.stamp) : '–' },
    ];
  });
  /** Whether a change is being saved. */
  protected readonly saving = signal(false);
  /** What the last change did; null before one. */
  protected readonly message = signal<StatsMessage | null>(null);

  /** Searches the stats files by the text typed (scenario names that hold it). */
  protected search(text: string): void {
    this.query.set(text);
  }

  /** Pairs the recording with one of KovaaK's stats files, or with none (null). */
  protected choose(file: string | null): void {
    this.choice({ file, source: 'kovaak' }, file ? `Paired with ${file}` : 'No stats file');
  }

  /** Back to finding the stats file by the recording's name and time. */
  protected findAgain(): void {
    this.choice({ auto: true }, 'Found by its name and time again');
  }

  /** Pairs the recording with a .csv chosen from this computer. */
  protected pickFile(input: HTMLInputElement): void {
    const file = input.files?.[0];
    input.value = '';
    const id = this.recording().id;
    if (file) void this.save(() => this.stats.pairFile(id, file), `Paired with ${file.name}`);
  }

  /** Saves a choice of stats file for the recording; `done` says what it did. */
  private choice(choice: StatsChoice, done: string): void {
    const id = this.recording().id;
    void this.save(() => this.stats.choose(id, choice), done);
  }

  /**
   * Makes a change, then shows it: the recording's stats flag, the list's row, the review measured
   * again, and what it did (or why it failed).
   */
  private async save(change: () => Promise<StatsChange>, done: string): Promise<void> {
    const id = this.recording().id;
    this.saving.set(true);
    this.message.set(null);
    try {
      const made = await change();
      this.library.source.patch(id, { stats: made.stats });
      if (this.library.selectedId() === id) this.review.follow(made.job);
      this.pairing.reload();
      const measuring = made.job.stage === 'none' ? '' : '; the review is measured again';
      this.message.set({ text: `${done}${measuring}`, failed: false });
    } catch (error) {
      this.message.set({ text: `Could not save: ${errorMessage(error)}`, failed: true });
    } finally {
      this.saving.set(false);
    }
  }
}
