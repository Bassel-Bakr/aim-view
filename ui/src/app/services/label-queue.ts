import { computed, effect, inject, Service, signal, untracked } from '@angular/core';
import { errorMessage } from '../api';
import { Labelling } from '../platform/labelling';
import { Playback } from '../run/playback';
import { Library } from './library';

/** Where the queue opens each recording, in seconds: a frame from the run, not the countdown. */
const RUN_FRAME = 20;

/** What the queue last said: it was empty or went through, or a step failed. */
export interface QueueNote {
  text: string;
  failed: boolean;
}

/**
 * The area labelling queue (this mode's Labelling): recordings opened one by one, uploads first, then one per
 * scenario, each at a frame from the run. The areas editor saves a recording's areas and moves on (next); the user
 * can skip one (it leaves the queue for good) or mark it as another game. Opening another recording ends the queue.
 */
@Service()
export class LabelQueue {
  private readonly labelling = inject(Labelling);
  private readonly library = inject(Library);
  private readonly playback = inject(Playback);
  private readonly ids = signal<readonly string[] | null>(null);
  readonly position = signal(0);
  /** Going through the queue. */
  readonly active = computed(() => this.ids() !== null);
  /** How many recordings the queue holds. */
  readonly length = computed(() => this.ids()?.length ?? 0);
  /** The recording being labelled; null when not labelling. */
  readonly current = computed(() => this.ids()?.[this.position()] ?? null);
  /** The recording after it, whose areas can be found while this one is checked; null at the end. */
  readonly upcoming = computed(() => this.ids()?.[this.position() + 1] ?? null);
  /** The queue is being read. */
  readonly loading = signal(false);
  readonly note = signal<QueueNote | null>(null);
  /** The area finder's training data kept here, to download and load; null where the review server keeps it. */
  readonly examples = this.labelling.examples;

  constructor() {
    effect(() => {
      const open = this.library.selectedId();
      if (open !== untracked(this.current)) untracked(() => this.stop());
    });
  }

  /** Reads the queue and opens its first recording. */
  async start(): Promise<void> {
    this.note.set(null);
    this.loading.set(true);
    try {
      const ids = await this.labelling.queue();
      if (!ids.length) {
        this.say(
          this.library.all().length
            ? 'Every recording has saved areas already (or is skipped, a probe or another game)'
            : 'No recordings to label: open or add some first',
        );
        return;
      }
      this.ids.set(ids);
      this.position.set(0);
      this.open();
    } catch (e) {
      this.say(`Could not read the labelling queue: ${errorMessage(e)}`, true);
    } finally {
      this.loading.set(false);
    }
  }

  /** Opens the next recording (the areas editor calls it once it saved this one's areas); after the last, ends. */
  next(): void {
    const ids = this.ids();
    if (!ids) return;
    if (this.position() + 1 >= ids.length) {
      this.stop();
      this.say(`Went through all ${ids.length} recordings`);
      return;
    }
    this.position.update((at) => at + 1);
    this.open();
  }

  /** Leaves the open recording out of the queue from now on, and opens the next. */
  async skip(): Promise<void> {
    const id = this.current();
    if (!id) return;
    try {
      await this.labelling.skip(id);
    } catch (e) {
      this.say(`Could not keep the skip: ${errorMessage(e)}`, true);
    }
    if (this.current() === id) this.next();
  }

  /**
   * Marks a recording as another game (on: it leaves the queues and what the area finder learns), or as an aim
   * trainer again. Marking the queue's recording opens the next.
   */
  async setNotAim(id: string, on: boolean): Promise<void> {
    try {
      await this.labelling.setNotAim(id, on);
    } catch (e) {
      this.say(`Could not mark it: ${errorMessage(e)}`, true);
      return;
    }
    if (on && this.current() === id) return this.next();
    this.say(
      on
        ? 'Marked as another game: left out of labelling and learning'
        : 'Marked as an aim trainer again',
    );
  }

  /** Ends the queue; the open recording stays open. */
  stop(): void {
    this.ids.set(null);
    this.position.set(0);
    this.playback.startAt = null;
  }

  /** Opens the queue's recording at a frame from the run. */
  private open(): void {
    const id = this.current();
    if (!id) return;
    this.playback.startAt = RUN_FRAME;
    if (this.library.selectedId() === id) this.playback.seek(RUN_FRAME);
    else this.library.selectedId.set(id);
  }

  private say(text: string, failed = false): void {
    this.note.set({ text, failed });
  }
}
