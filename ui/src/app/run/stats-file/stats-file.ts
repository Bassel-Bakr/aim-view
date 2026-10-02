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
import { StatsFiles, StatsSetup } from '../../platform/stats-files';
import { Library } from '../../services/library';
import { Review } from '../../services/review';
import { badge, button } from '@themes/controls.styles';
import { statsFileStyles } from '@themes/stats-file.styles';
import { slotClasses } from '@themes/slot-classes';

const HOW: Record<StatsHow, string> = {
  picked: 'your pick',
  upload: 'chosen by you from this computer',
  none: 'you chose none',
  gone: 'your pick, but the file is gone',
  beside: 'uploaded with the recording',
  found: 'found by its name and time',
  missing: 'none found by its name and time',
};

/** A stats file the panel offers: when it was written, how far from the recording, and its scenario if another. */
export interface CandidateRow {
  name: string;
  when: string;
  off: string;
  scenario: string | null;
  inUse: boolean;
}

/** One fact from a stats file, as the panel lists it. */
export interface StatsFact {
  label: string;
  value: string;
}

/** What the last change did; failed when it could not be made. */
export interface StatsMessage {
  text: string;
  failed: boolean;
}

/** A pairing answer, with the recording it is for. */
export interface PairingFor {
  id: string;
  pairing: StatsPairing | undefined;
}

/**
 * Pairs the open recording with a stats file, through this mode's StatsFiles: a .csv from this computer, none, or
 * (where it reaches KovaaK's stats folder) one of its files, searched by scenario and offered nearest the recording's
 * time first. A reviewed recording is then measured again with it.
 */
@Component({
  selector: 'app-stats-file',
  templateUrl: './stats-file.html',
})
export class StatsFile {
  readonly recording = input.required<Recording>();
  readonly closed = output();
  protected readonly stats = inject(StatsFiles);
  protected readonly library = inject(Library);
  private readonly review = inject(Review);
  protected readonly ui = slotClasses(statsFileStyles());
  protected readonly button = button();
  protected readonly primaryButton = button({ intent: 'primary' });
  protected readonly goodBadge = badge({ tone: 'good' });

  /** The search text; null: the recording's own scenario. A newly opened recording starts again from its own. */
  protected readonly query = linkedSignal<string, string | null>({
    source: () => this.recording().id,
    computation: () => null,
  });
  protected readonly pairing = this.stats.pairing(
    () => this.recording().id,
    () => this.query(),
  );
  /** The last answer for this recording, kept while a new search loads, so the panel does not flicker. */
  protected readonly shown = linkedSignal<PairingFor, StatsPairing | null>({
    source: () => ({
      id: this.recording().id,
      pairing: this.pairing.hasValue() ? this.pairing.value() : undefined,
    }),
    computation: (now, previous) =>
      now.pairing ?? (previous?.source.id === now.id ? previous.value : null),
  });
  protected readonly how = computed(() => {
    const p = this.shown();
    return p ? HOW[p.how] : '';
  });
  /** The user chose something, so finding it by name and time again changes something. */
  protected readonly chosen = computed(() =>
    ['picked', 'upload', 'none', 'gone'].includes(this.shown()?.how ?? ''),
  );
  protected readonly rows = computed<CandidateRow[]>(() => {
    const p = this.shown();
    if (!p) return [];
    return p.candidates.map((c) => ({
      name: c.name,
      when: formatStamp(c.stamp),
      off: formatOffset(c.off),
      scenario: c.scenario.toLowerCase() === p.scenario.toLowerCase() ? null : c.scenario,
      inUse: c.name === p.file,
    }));
  });
  /** What the stats file says, when it was read where the page runs. */
  protected readonly facts = computed<StatsFact[] | null>(() => {
    const s = this.shown()?.facts;
    if (!s) return null;
    return [
      { label: 'Scenario', value: s.scenario ?? '–' },
      { label: 'Score', value: s.score === null ? '–' : formatNumber(s.score) },
      { label: 'Kills', value: formatCount(s.kills) },
      { label: 'Accuracy', value: formatPercent(s.accuracy) },
      { label: 'Ended', value: s.stamp ? formatStamp(s.stamp) : '–' },
    ];
  });
  protected readonly saving = signal(false);
  protected readonly message = signal<StatsMessage | null>(null);

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

  /** Opens the stats folder, or asks for leave to read it again (in the click, as the browser needs). */
  protected runSetup(setup: StatsSetup): void {
    void this.act(() => setup.run(), "KovaaK's stats folder is open");
  }

  /** The stats folder chosen as files (where the browser's folder picker cannot open it). */
  protected pickFolder(input: HTMLInputElement, setup: StatsSetup): void {
    const files = [...(input.files ?? [])];
    input.value = '';
    const read = setup.files;
    if (files.length && read) void this.act(() => read(files), "KovaaK's stats folder is read");
  }

  /** Runs a step that changes no file by itself, showing what it did or why it failed. */
  private async act(step: () => Promise<void>, done: string): Promise<void> {
    this.saving.set(true);
    this.message.set(null);
    try {
      await step();
      this.pairing.reload();
      this.message.set({ text: done, failed: false });
    } catch (e) {
      this.message.set({ text: errorMessage(e), failed: true });
    } finally {
      this.saving.set(false);
    }
  }

  /** Pairs the recording with a .csv chosen from this computer. */
  protected pickFile(input: HTMLInputElement): void {
    const file = input.files?.[0];
    input.value = '';
    const id = this.recording().id;
    if (file) void this.save(() => this.stats.pairFile(id, file), `Paired with ${file.name}`);
  }

  private choice(choice: StatsChoice, done: string): void {
    const id = this.recording().id;
    void this.save(() => this.stats.choose(id, choice), done);
  }

  /** Makes a change, then shows it: the list's row, the review measured again, and what it did. */
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
    } catch (e) {
      this.message.set({ text: `Could not save: ${errorMessage(e)}`, failed: true });
    } finally {
      this.saving.set(false);
    }
  }
}
