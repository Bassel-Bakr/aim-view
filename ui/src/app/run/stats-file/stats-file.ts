import { HttpClient, httpResource } from '@angular/common/http';
import { Component, computed, inject, input, linkedSignal, output, signal } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import {
  errorMessage,
  Recording,
  StatsChange,
  StatsChoice,
  StatsHow,
  StatsPairing,
} from '../../api';
import { formatCount, formatNumber, formatOffset, formatPercent, formatStamp } from '../../format';
import { Library } from '../../services/library';
import { isLocal, LocalFiles } from '../../services/local-files';
import { Review } from '../../services/review';
import { statsSummary } from '../../services/stats-csv';
import { badge, button } from '@themes/controls.styles';
import { statsFileStyles } from '@themes/stats-file.styles';
import { slotClasses } from '@themes/slot-classes';

const HOW: Record<StatsHow, string> = {
  picked: 'your pick',
  upload: 'uploaded by you',
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

/** The stats file a recording from this computer has, and what it says. */
export interface LocalStats {
  name: string;
  facts: StatsFact[];
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
 * Pairs the open recording with a stats file. The review server's recordings: one of KovaaK's stats files, searched
 * by scenario and offered nearest the recording's time first, or none; the review is then measured again with it.
 * A recording from this computer: a .csv chosen from this computer, read in the browser.
 */
@Component({
  selector: 'app-stats-file',
  templateUrl: './stats-file.html',
})
export class StatsFile {
  readonly recording = input.required<Recording>();
  readonly closed = output();
  private readonly http = inject(HttpClient);
  private readonly library = inject(Library);
  private readonly review = inject(Review);
  private readonly local = inject(LocalFiles);
  protected readonly ui = slotClasses(statsFileStyles());
  protected readonly button = button();
  protected readonly goodBadge = badge({ tone: 'good' });

  protected readonly isLocal = computed(() => isLocal(this.recording().id));
  /** The search text; null: the recording's own scenario. A newly opened recording starts again from its own. */
  protected readonly query = linkedSignal<string, string | null>({
    source: () => this.recording().id,
    computation: () => null,
  });
  protected readonly pairing = httpResource<StatsPairing>(() => {
    const id = this.recording().id;
    if (isLocal(id)) return undefined;
    const q = this.query();
    const params: Record<string, string> = q === null ? { id } : { id, q };
    return { url: '/api/stats', params };
  });
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
  protected readonly localStats = computed<LocalStats | null>(() => {
    const stats = this.local.find(this.recording().id)?.stats;
    if (!stats) return null;
    const s = statsSummary(stats);
    return {
      name: stats.name,
      facts: [
        { label: 'Scenario', value: s.scenario ?? '–' },
        { label: 'Score', value: s.score === null ? '–' : formatNumber(s.score) },
        { label: 'Kills', value: formatCount(s.kills) },
        { label: 'Accuracy', value: formatPercent(s.accuracy) },
        { label: 'Ended', value: s.stamp ? formatStamp(s.stamp) : '–' },
      ],
    };
  });
  protected readonly saving = signal(false);
  protected readonly message = signal<StatsMessage | null>(null);

  protected search(text: string): void {
    this.query.set(text);
  }

  /** Pairs the recording with one of KovaaK's stats files, or with none (null). */
  protected choose(file: string | null): void {
    void this.save({ file, source: 'kovaak' }, file ? `Paired with ${file}` : 'No stats file');
  }

  /** Back to finding the stats file by the recording's name and time. */
  protected findAgain(): void {
    void this.save({ auto: true }, 'Found by its name and time again');
  }

  protected async pickLocal(input: HTMLInputElement): Promise<void> {
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    const ok = await this.local.pair(this.recording().id, file);
    this.message.set(
      ok
        ? { text: `Paired with ${file.name}`, failed: false }
        : { text: `${file.name} is not one of KovaaK's stats files`, failed: true },
    );
  }

  protected removeLocal(): void {
    this.local.unpair(this.recording().id);
    this.message.set({ text: 'No stats file', failed: false });
  }

  private async save(choice: StatsChoice, done: string): Promise<void> {
    const id = this.recording().id;
    this.saving.set(true);
    this.message.set(null);
    try {
      const change = await firstValueFrom(
        this.http.post<StatsChange>('/api/stats', choice, { params: { id } }),
      );
      this.library.recordings.update((list) =>
        list?.map((r) => (r.id === id ? { ...r, stats: change.stats } : r)),
      );
      if (this.library.selectedId() === id) this.review.follow(change.job);
      this.pairing.reload();
      const measuring = change.job.stage === 'none' ? '' : '; the review is measured again';
      this.message.set({ text: `${done}${measuring}`, failed: false });
    } catch (e) {
      this.message.set({ text: `Could not save: ${errorMessage(e)}`, failed: true });
    } finally {
      this.saving.set(false);
    }
  }
}
