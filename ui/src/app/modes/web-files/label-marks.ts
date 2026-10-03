import { inject, Injectable, signal } from '@angular/core';
import { exampleRec } from './area-examples';
import { BrowserStore } from './browser-store';

const NOT_AIM_KEY = 'not-aim';
const SKIPPED_KEY = 'label-skipped';

/**
 * The recordings the user marked as another game, and those skipped in the labelling queue, kept in this browser
 * (IndexedDB) as the review server keeps not_aim_trainer.json and label_skipped.json: by the recording's name in the
 * area examples (exampleRec), so a VOD folder's video keeps its marks across visits.
 */
@Injectable({ providedIn: 'root' })
export class LabelMarks {
  private readonly store = inject(BrowserStore);
  private readonly _notAim = signal<ReadonlySet<string>>(new Set());
  private readonly _skipped = signal<ReadonlySet<string>>(new Set());
  /** The recordings marked as another game, by exampleRec. */
  readonly notAim = this._notAim.asReadonly();
  /** The recordings skipped in the labelling queue, by exampleRec. */
  readonly skipped = this._skipped.asReadonly();
  /** The kept marks have been read from the store. */
  readonly ready: Promise<void>;

  constructor() {
    this.ready = Promise.all([
      this.store.get<string[]>(NOT_AIM_KEY),
      this.store.get<string[]>(SKIPPED_KEY),
    ])
      .then(([notAim, skipped]) => {
        if (notAim) this._notAim.update((now) => new Set([...notAim, ...now]));
        if (skipped) this._skipped.update((now) => new Set([...skipped, ...now]));
      })
      .catch(() => undefined);
  }

  async setNotAim(id: string, on: boolean): Promise<void> {
    await this.ready;
    this._notAim.update((now) => toggled(now, exampleRec(id), on));
    await this.store.set(NOT_AIM_KEY, [...this._notAim()].sort());
  }

  async skip(id: string): Promise<void> {
    await this.ready;
    this._skipped.update((now) => toggled(now, exampleRec(id), true));
    await this.store.set(SKIPPED_KEY, [...this._skipped()].sort());
  }
}

/** The set with the name in it (on) or out of it. */
function toggled(set: ReadonlySet<string>, name: string, on: boolean): ReadonlySet<string> {
  const next = new Set(set);
  if (on) next.add(name);
  else next.delete(name);
  return next;
}
