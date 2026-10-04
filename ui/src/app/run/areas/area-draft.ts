import { computed, effect, inject, Service, linkedSignal, signal, untracked } from '@angular/core';
import {
  AreaBox,
  AreaKind,
  AreaRect,
  AreaSource,
  errorMessage,
  FoundAreas,
  KeptAreas,
} from '../../api';
import { AreaLabels } from '../../platform/area-labels';
import { LabelQueue } from '../../services/label-queue';
import { Library } from '../../services/library';
import { Review } from '../../services/review';
import { Playback } from '../playback';
import { sameAreas } from './area-geometry';

/** What the editor is doing that takes a while: finding areas (copying or fresh), or saving them. */
export type AreaTask = 'find' | 'detect' | 'save';

/** The kind a new area has until the user names it. */
const OTHER = 'other';

const FROM: Record<AreaSource, string> = {
  saved: 'Saved for this recording.',
  'last upload': 'From the last added recording you saved areas for.',
  kovobs: 'KovOBS’s layout (the default).',
};

const plural = (count: number, word: string) => `${count} ${word}${count === 1 ? '' : 's'}`;

/** What the finder's proposal says: the recording it copied from, or how many areas it found and named how. */
export function foundNote(found: FoundAreas): string {
  const taught = `Learned from ${plural(found.recordings, 'recording')} you saved.`;
  if (found.copied) {
    const from = found.copied.replace(/_/g, ' ');
    return `Same layout as ${from}: your areas from there. ${taught} Check them, then Save.`;
  }
  return (
    `Found ${plural(found.boxes.length, 'area')}: ${found.by.learned ?? 0} named from what you taught, ` +
    `${found.by.rule ?? 0} by rules. ${taught} Check them, then Save.`
  );
}

/**
 * The open recording's excluded areas while the user edits them: the areas drawn over the video, the selected one, the
 * kinds, and what the editor last said. Saving keeps them (the area finder learns from them) and, where the review
 * was tracked with other areas, tracks it again. While the labelling queue is on the open recording, the editor opens
 * with the finder's proposal, and saving moves on to the queue's next recording.
 */
@Service()
export class AreaDraft {
  private readonly labels = inject(AreaLabels);
  private readonly library = inject(Library);
  private readonly review = inject(Review);
  private readonly playback = inject(Playback);
  private readonly queue = inject(LabelQueue);

  /** The recording whose areas are being edited; null: the editor is closed. */
  readonly editing = signal<string | null>(null);
  readonly open = computed(() => this.editing() !== null);
  /** Why the finder cannot run in this mode; null when it can. */
  readonly finderMissing = this.labels.finderMissing;
  /** The areas as kept for the recording (or where they start from), with the kinds. */
  private readonly kept = this.labels.areas(() => this.editing() ?? undefined);
  private readonly keptValue = computed(() => (this.kept.hasValue() ? this.kept.value() : null));
  /**
   * The kept areas, and where they come from: read again with the same areas (the kinds changed), they stay the same,
   * so the areas on screen are not reset.
   */
  private readonly keptBoxes = computed(() => this.keptValue()?.boxes ?? null, {
    equal: (a, b) => JSON.stringify(a) === JSON.stringify(b),
  });
  private readonly keptSource = computed(() => this.keptValue()?.source ?? null);
  /** The areas on screen. */
  readonly boxes = linkedSignal<AreaBox[]>(() =>
    (this.keptBoxes() ?? []).map((b): AreaBox => [...b]),
  );
  /** The kinds, as the mode keeps them (they follow a kinds file loaded in the browser). */
  readonly kinds = linkedSignal<AreaKind[]>(() => this.keptValue()?.kinds ?? []);
  /** The selected area's index; -1: none. */
  readonly selected = signal(-1);
  /** What the editor says: where the areas come from, what the finder did, or what failed. */
  readonly note = linkedSignal<string>(() => {
    const source = this.keptSource();
    if (source) return FROM[source] ?? '';
    return this.kept.error() ? `Could not read the areas: ${errorMessage(this.kept.error())}` : '';
  });
  readonly busy = signal<AreaTask | null>(null);
  /** The areas are read and can be changed. */
  readonly ready = computed(() => this.keptValue() !== null);
  /** The labelling queue is on the recording being edited: saving moves on to the next one. */
  readonly queued = computed(() => {
    const id = this.editing();
    return id !== null && this.queue.current() === id;
  });
  /** The queue's recording the finder last proposed areas for. */
  private proposed: string | null = null;
  /** The labelling queue opened the editor: it closes when the queue ends. */
  private fromQueue = false;

  constructor() {
    // another recording opened: the editor closes
    effect(() => {
      const open = this.library.selectedId();
      const id = untracked(this.editing);
      if (id !== null && id !== open) untracked(() => this.stop());
    });
    // the labelling queue's recording opens in the editor; the next one's areas are found meanwhile
    effect(() => {
      const id = this.queue.current();
      if (id === null || this.library.selectedId() !== id) return;
      untracked(() => {
        if (this.editing() !== id) this.start(id);
        this.fromQueue = true;
        const next = this.queue.upcoming();
        if (next && !this.finderMissing) this.labels.find(next, true).catch(() => undefined);
      });
    });
    // once its areas are read, the finder's proposal takes their place (once a recording)
    effect(() => {
      if (!this.queued() || !this.ready()) return;
      const id = untracked(this.editing);
      if (id === null || id === this.proposed) return;
      this.proposed = id;
      untracked(() => void this.find(true));
    });
    // the queue ended (went through, or was stopped): the editor it opened closes
    effect(() => {
      if (this.queue.active() || !this.fromQueue) return;
      this.fromQueue = false;
      untracked(() => this.stop());
    });
  }

  /** Opens the editor on a recording: the video pauses, the review's overlay makes way for the areas. */
  start(id: string): void {
    this.playback.pause();
    this.selected.set(-1);
    this.busy.set(null);
    if (this.editing() === id) {
      this.kept.reload();
      return;
    }
    this.editing.set(id);
  }

  /** Closes the editor; what was not saved is dropped. */
  stop(): void {
    this.editing.set(null);
    this.selected.set(-1);
    this.busy.set(null);
  }

  /** Cancel: closes the editor, and ends the labelling queue when the editor is on its recording. */
  cancel(): void {
    if (this.queued()) this.queue.stop();
    this.stop();
  }

  select(index: number): void {
    this.selected.set(index < this.boxes().length ? index : -1);
  }

  /** The kind's name, or its id where no kind has it. */
  kindName(id: string): string {
    return this.kinds().find((candidate) => candidate.id === id)?.name ?? id;
  }

  /** A new area, selected so the user can say what it is. */
  add(rect: AreaRect): void {
    this.boxes.update((list) => [...list, [...rect, OTHER]]);
    this.selected.set(this.boxes().length - 1);
  }

  /** An area moved or resized. */
  place(index: number, [x0, y0, x1, y1]: AreaRect): void {
    this.boxes.update((list) =>
      list.map((box, i): AreaBox => (i === index ? [x0, y0, x1, y1, box[4]] : box)),
    );
  }

  /** The selected area's kind. */
  setKind(kind: string): void {
    const at = this.selected();
    if (at < 0) return;
    this.boxes.update((list) =>
      list.map((box, i): AreaBox => (i === at ? [box[0], box[1], box[2], box[3], kind] : box)),
    );
  }

  /** Removes the selected area. */
  remove(): void {
    const at = this.selected();
    if (at < 0) return;
    this.boxes.update((list) => list.filter((_box, i) => i !== at));
    this.selected.set(-1);
  }

  clear(): void {
    this.boxes.set([]);
    this.selected.set(-1);
  }

  /** KovOBS's layout in place of the areas on screen. */
  async useLayout(): Promise<void> {
    const id = this.editing();
    try {
      const layout = await this.labels.layout();
      if (this.editing() !== id) return;
      this.boxes.set(layout.boxes);
      this.selected.set(-1);
      this.note.set(`${FROM.kovobs} Check them, then Save.`);
    } catch (error) {
      this.note.set(`Could not read KovOBS’s layout: ${errorMessage(error)}`);
    }
  }

  /**
   * The finder's proposal in place of the areas on screen: with copy, the user's own areas from a recording with the
   * same layout when there is one (Find areas); else the areas found in this one (Detect fresh).
   */
  async find(copy: boolean): Promise<void> {
    const id = this.editing();
    if (id === null) return;
    if (this.finderMissing) {
      this.note.set(this.finderMissing);
      return;
    }
    this.busy.set(copy ? 'find' : 'detect');
    try {
      const found = await this.labels.find(id, copy);
      if (this.editing() !== id) return;
      this.boxes.set(found.boxes.map((b): AreaBox => [...b]));
      this.selected.set(-1);
      this.note.set(foundNote(found));
    } catch (error) {
      if (this.editing() === id) this.note.set(`Could not find areas: ${errorMessage(error)}`);
    } finally {
      if (this.editing() === id) this.busy.set(null);
    }
  }

  /**
   * Keeps the areas, and the finder learns from them. A reviewed recording whose review was tracked with other areas
   * (or with areas the review does not say) is tracked again with them. Then the editor closes, or in the labelling
   * queue the next recording opens.
   */
  async save(): Promise<void> {
    const id = this.editing();
    if (id === null) return;
    this.busy.set('save');
    try {
      const kept = await this.labels.save(id, this.boxes());
      if (this.editing() !== id) return;
      this.trackAgain(kept);
      const queued = this.queued();
      this.stop();
      if (queued) this.queue.next();
    } catch (error) {
      if (this.editing() === id) this.note.set(`Could not save the areas: ${errorMessage(error)}`);
    } finally {
      if (this.busy() === 'save') this.busy.set(null);
    }
  }

  /**
   * A reviewed recording whose review was tracked with other areas (or with areas the review does not say) is tracked
   * again with the kept ones, unless the service already started that.
   */
  private trackAgain(kept: KeptAreas): void {
    if (kept.job && kept.job.stage !== 'none') {
      this.review.follow(kept.job);
      return;
    }
    const reviewed = this.library.selected()?.analysed ?? false;
    const tracks = this.review.tracks.hasValue() ? this.review.tracks.value() : null;
    const tracked = tracks?.areas;
    if (reviewed && !(tracked && sameAreas(tracked, kept.boxes))) void this.review.analyse(true);
  }

  /**
   * Adds a kind (id null), or gives one a new name and description; a new kind becomes the selected area's. Resolves
   * to whether it was kept (else the note says why).
   */
  async saveKind(id: string | null, name: string, about: string): Promise<boolean> {
    try {
      const kinds = await this.labels.saveKind({ id, name, about });
      this.kinds.set(kinds);
      if (id === null && this.selected() >= 0) {
        const wanted = name.trim().toLowerCase();
        this.setKind(
          kinds.find((candidate) => candidate.name.toLowerCase() === wanted)?.id ?? OTHER,
        );
      }
      return true;
    } catch (error) {
      this.note.set(`Could not save the type: ${errorMessage(error)}`);
      return false;
    }
  }
}
