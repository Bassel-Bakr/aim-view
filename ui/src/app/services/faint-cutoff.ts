import { computed, effect, inject, Service, linkedSignal, signal, untracked } from '@angular/core';
import { errorMessage, FaintChoice, FaintSetting, Tracks } from '../api';
import { FaintCutoffs } from '../platform/faint-cutoffs';
import {
  FaintScores,
  faintScores,
  tracksUnder,
  tracksWithout,
} from '../run/faint-cutoff/faint-scores';
import { Library } from './library';
import { Review } from './review';

/** The offset a cut-off starts at, and how long the controls wait for the user to stop before it is saved. */
const DEFAULT_OFFSET = 0.3;
const SAVE_MS = 400;
/** A tracking run counts the scores near the crosshair (its bot is under it); a clicking run leaves 2 degrees out. */
const CLICK_NEAR = 2;

/** What the cut-off last said: a submit, or a step that failed. */
export interface FaintNote {
  text: string;
  failed: boolean;
}

/** The point under the mouse on the video: its track, its place on the canvas, and the frame it was read from. */
export interface FaintHover {
  id: number;
  x: number;
  y: number;
  frame: number;
  text: string;
}

/** A change waiting for the controls to rest: the recording, the cut-off, and its timer. */
interface PendingSave {
  id: string;
  choice: FaintChoice;
  timer: ReturnType<typeof setTimeout>;
}

/** The saved cut-off of the recording it belongs to. */
interface SavedOf {
  id: string | null;
  setting: FaintSetting | undefined;
}

/**
 * The open recording's faint-target cut-off (this mode's FaintCutoffs): the user's setting, each track's score and the
 * tracks the cut leaves out. The page shows and measures the tracks without them: a tracking run's report is measured
 * again with the cut (by the server, or the core in the browser), the overlay, the timeline and a clicking run's
 * fastest paths leave them out at once, and the video shows them dimmed. Submit writes the cut as detector labels.
 */
@Service()
export class FaintCutoff {
  private readonly cutoffs = inject(FaintCutoffs);
  private readonly library = inject(Library);
  private readonly review = inject(Review);
  /** The cut-off as saved for the open recording. */
  readonly saved = this.cutoffs.setting(() => this.library.selectedId() ?? undefined);
  private readonly savedOf = computed<SavedOf>(() => ({
    id: this.library.selectedId(),
    setting: this.saved.hasValue() ? this.saved.value() : undefined,
  }));
  /** On or off, as the user set it: the saved setting until changed (kept while it reloads). */
  readonly on = linkedSignal<SavedOf, boolean>({
    source: this.savedOf,
    computation: (s, prev) =>
      s.setting?.on ?? (prev && prev.source.id === s.id ? prev.value : false),
  });
  readonly offset = linkedSignal<SavedOf, number>({
    source: this.savedOf,
    computation: (s, prev) =>
      s.setting?.offset ?? (prev && prev.source.id === s.id ? prev.value : DEFAULT_OFFSET),
  });
  /** The panel was asked for (Cut-off on the run page). */
  readonly asked = signal(false);
  /** Every track's score written beside it on the video. */
  readonly showScores = signal(false);
  /** The track picked in the strip, drawn highlighted on the video. */
  readonly highlight = signal<number | null>(null);
  readonly hover = signal<FaintHover | null>(null);
  readonly note = signal<FaintNote | null>(null);
  readonly submitting = signal(false);

  /** Every target of the review, the cut-off's tracks among them; null while there is none. */
  readonly allTracks = computed<Tracks | null>(() =>
    this.review.tracks.hasValue() ? (this.review.tracks.value() ?? null) : null,
  );
  private readonly near = computed(() => {
    const r = this.review.report.hasValue() ? this.review.report.value() : null;
    return r?.mode === 'track' ? 0 : CLICK_NEAR;
  });
  readonly scores = computed<FaintScores | null>(() => {
    const t = this.allTracks();
    return t ? faintScores(t.frames, this.near()) : null;
  });
  /** The review has the detector's scores: the cut-off can work. */
  readonly has = computed(() => this.scores()?.level != null);
  /** The score the cut-off cuts at: the recording's level less the offset. */
  readonly cut = computed(() => (this.scores()?.level ?? 0) - this.offset());
  /** The tracks scoring under the cut, which the cut-off leaves out when it is on. */
  readonly under = computed(() => {
    const sc = this.scores();
    return sc && this.has() ? tracksUnder(sc, this.cut()) : new Set<number>();
  });
  readonly dropped = computed(() => (this.on() ? this.under() : new Set<number>()));
  /** The tracks the page shows and measures: without those the cut-off leaves out. */
  readonly tracks = computed<Tracks | null>(() => {
    const t = this.allTracks();
    return t && tracksWithout(t, this.dropped());
  });
  /** The open recording is the cut-off queue's (FaintQueue sets it). */
  readonly queued = signal(false);
  /** The panel shows when asked for, while the cut-off is on, or in the cut-off queue. */
  readonly shown = computed(() => this.asked() || this.on() || this.queued());

  private pending: PendingSave | null = null;
  private saving: Promise<void> = Promise.resolve();

  constructor() {
    effect(() => {
      this.library.selectedId();
      untracked(() => {
        this.flush();
        this.highlight.set(null);
        this.hover.set(null);
        this.note.set(null);
      });
    });
  }

  /** The user's change: shown at once, saved once the controls rest. */
  change(choice: FaintChoice): void {
    if (!this.has()) return;
    this.on.set(choice.on);
    this.offset.set(choice.offset);
    const id = this.library.selectedId();
    if (!id) return;
    if (this.pending) clearTimeout(this.pending.timer);
    this.pending = { id, choice, timer: setTimeout(() => void this.flush(), SAVE_MS) };
  }

  /** Saves a change still waiting, at once. */
  private flush(): Promise<void> {
    const p = this.pending;
    if (p) {
      clearTimeout(p.timer);
      this.pending = null;
      this.saving = this.save(p.id, p.choice);
    }
    return this.saving;
  }

  /** Keeps the cut-off; a tracking run's report is measured again with it (the job, followed). */
  private async save(id: string, choice: FaintChoice): Promise<void> {
    try {
      const job = await this.cutoffs.save(id, choice);
      if (this.library.selectedId() !== id) return;
      // a measure the save started is followed; one already done (or a clicking run's last job) reloads the report
      if (!['none', 'done', 'error'].includes(job.stage)) this.review.follow(job);
      else if (job.stage === 'done') this.review.report.reload();
    } catch (e) {
      if (this.library.selectedId() === id)
        this.say(`Could not save the cut-off: ${errorMessage(e)}`, true);
    }
  }

  /** Submits the cut-off: kept (on), and written as detector labels. Resolves to whether it went. */
  async submit(): Promise<boolean> {
    const id = this.library.selectedId();
    if (!id || !this.has()) return false;
    this.submitting.set(true);
    try {
      await this.flush();
      await this.cutoffs.submit(id, this.offset());
      this.on.set(true);
      this.saved.reload();
      this.say(`Submitted: cut ${this.cut().toFixed(2)} saved; its labels are being written`);
      return true;
    } catch (e) {
      this.say(`Could not submit: ${errorMessage(e)}`, true);
      return false;
    } finally {
      this.submitting.set(false);
    }
  }

  private say(text: string, failed = false): void {
    this.note.set({ text, failed });
  }
}
