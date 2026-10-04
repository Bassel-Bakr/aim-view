import { computed, effect, inject, Service, signal, untracked } from '@angular/core';
import { errorMessage } from '../api';
import { FaintCutoffs } from '../platform/faint-cutoffs';
import { FaintCutoff, FaintNote } from './faint-cutoff';
import { Library } from './library';
import { Review } from './review';

/**
 * The cut-off queue (this mode's FaintCutoffs): recordings opened one by one, in the area queue's order, to set the
 * faint-target cut-off in each, Submit and next. A recording not reviewed yet is reviewed first, and one reviewed before
 * the detector's scores were kept is reviewed again (the scores come with the review). Skip leaves one out for good.
 * Opening another recording ends the queue.
 */
@Service()
export class FaintQueue {
  private readonly cutoffs = inject(FaintCutoffs);
  private readonly faint = inject(FaintCutoff);
  private readonly library = inject(Library);
  private readonly review = inject(Review);
  private readonly ids = signal<readonly string[] | null>(null);
  readonly position = signal(0);
  readonly active = computed(() => this.ids() !== null);
  readonly length = computed(() => this.ids()?.length ?? 0);
  /** The recording the queue has open; null when not going through it. */
  readonly current = computed(() => this.ids()?.[this.position()] ?? null);
  readonly loading = signal(false);
  readonly note = signal<FaintNote | null>(null);
  /** The recordings reviewed again for their scores, so each is reviewed again once. */
  private readonly reviewedAgain = new Set<string>();

  constructor() {
    effect(() => {
      const open = this.library.selectedId();
      if (open !== untracked(this.current)) untracked(() => this.stop());
    });
    effect(() => {
      const id = this.current();
      this.faint.queued.set(id !== null && id === this.library.selectedId());
    });
    // a review without the detector's scores: reviewed again, once, for them
    effect(() => {
      const id = this.current();
      const tracks = this.faint.allTracks();
      if (!id || id !== this.library.selectedId() || !tracks || this.faint.has()) return;
      if (this.review.running() || this.reviewedAgain.has(id)) return;
      this.reviewedAgain.add(id);
      untracked(() => void this.review.analyse(true));
    });
  }

  /** Reads the queue and opens its first recording. */
  async start(): Promise<void> {
    this.note.set(null);
    this.loading.set(true);
    try {
      const ids = await this.cutoffs.queue();
      if (!ids.length) {
        this.say(
          this.library.all().length
            ? 'Every recording has a submitted cut-off already (or is skipped, a probe or another game)'
            : 'No recordings to set a cut-off in: open or add some first',
        );
        return;
      }
      this.ids.set(ids);
      this.position.set(0);
      this.open();
    } catch (e) {
      this.say(`Could not read the cut-off queue: ${errorMessage(e)}`, true);
    } finally {
      this.loading.set(false);
    }
  }

  /** Submits the open recording's cut-off, then opens the next. */
  async submitAndNext(): Promise<void> {
    if (await this.faint.submit()) this.next();
  }

  /** Leaves the open recording out of the queue from now on, and opens the next. */
  async skip(): Promise<void> {
    const id = this.current();
    if (!id) return;
    try {
      await this.cutoffs.skip(id);
    } catch (e) {
      this.say(`Could not keep the skip: ${errorMessage(e)}`, true);
    }
    if (this.current() === id) this.next();
  }

  /** Ends the queue; the open recording stays open. */
  stop(): void {
    this.ids.set(null);
    this.position.set(0);
  }

  /** Opens the next recording; after the last, ends. */
  private next(): void {
    const ids = this.ids();
    if (!ids) return;
    if (this.position() + 1 >= ids.length) {
      this.stop();
      this.say(
        ids.length === 1
          ? 'Went through the one recording'
          : `Went through all ${ids.length} recordings`,
      );
      return;
    }
    this.position.update((at) => at + 1);
    this.open();
  }

  /** Opens the queue's recording; one not reviewed yet is reviewed (its scores come with the review). */
  private open(): void {
    const id = this.current();
    if (!id) return;
    this.library.selectedId.set(id);
    const r = this.library.source.recordings().find((x) => x.id === id);
    if (r && !r.analysed) void this.review.analyse(false);
  }

  private say(text: string, failed = false): void {
    this.note.set({ text, failed });
  }
}
