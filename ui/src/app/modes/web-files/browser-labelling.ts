import { inject, Injectable } from '@angular/core';
import { Recording } from '../../api';
import { ExamplesStore, Labelling } from '../../platform/labelling';
import { AreaExamples, EXAMPLES_FILE, exampleRec, KINDS_FILE } from './area-examples';
import { LabelMarks } from './label-marks';
import { LocalFiles } from './local-files';

/** A file added from this computer: the browser's upload. */
const isAdded = (r: Recording) => exampleRec(r.id).startsWith('uploads/');

/**
 * What groups recordings in the queue: an added file on its own, a VOD folder's video by its folder (KovOBS keeps one
 * folder per scenario), or by its scenario when it lies at the top of the folder opened.
 */
function scenarioKey(r: Recording): string {
  const rec = exampleRec(r.id);
  if (isAdded(r)) return rec;
  const cut = rec.lastIndexOf('/');
  return cut < 0 ? `scenario:${r.scenario}` : rec.slice(0, cut);
}

/**
 * The recordings to label areas in, by python/server.py's rules (`label_queue`): added files first, then newest
 * first, one recording of each scenario, leaving out probes, other games, skipped ones and those with saved areas.
 */
export function labelQueue(
  recordings: readonly Recording[],
  skipped: ReadonlySet<string>,
  labelled: ReadonlySet<string>,
): string[] {
  const order = [...recordings].sort(
    (a, b) => Number(isAdded(b)) - Number(isAdded(a)) || b.mtime - a.mtime,
  );
  const out: string[] = [];
  const seen = new Set<string>();
  for (const r of order) {
    const key = scenarioKey(r);
    const rec = exampleRec(r.id);
    if (seen.has(key) || key.includes('Probe') || r.not_aim) continue;
    if (skipped.has(rec) || labelled.has(rec)) continue;
    seen.add(key);
    out.push(r.id);
  }
  return out;
}

/**
 * Labelling in the browser: the queue worked out over the recordings opened here, and the marks and the area
 * finder's training data kept in this browser (LabelMarks, AreaExamples).
 */
@Injectable({ providedIn: 'root' })
export class BrowserLabelling implements Labelling {
  private readonly files = inject(LocalFiles);
  private readonly marks = inject(LabelMarks);
  private readonly areas = inject(AreaExamples);

  readonly examples: ExamplesStore = {
    count: this.areas.count,
    fileNames: [EXAMPLES_FILE, KINDS_FILE],
    file: (name) => (name === KINDS_FILE ? this.areas.kindsFile() : this.areas.examplesFile()),
    load: (files) => this.areas.load(files),
  };

  async queue(): Promise<string[]> {
    await Promise.all([this.marks.ready, this.areas.ready]);
    return labelQueue(this.files.recordings(), this.marks.skipped(), this.areas.labelled());
  }

  skip(id: string): Promise<void> {
    return this.marks.skip(id);
  }

  /** The recordings list reads the mark (LocalFiles), so the row follows. */
  setNotAim(id: string, on: boolean): Promise<void> {
    return this.marks.setNotAim(id, on);
  }
}
