import { computed, inject, Injectable, signal } from '@angular/core';
import { AreaExample, AreaKind } from '../../api';
import { ExamplesCount, ExamplesLoaded } from '../../platform/labelling';
import { builtInKinds } from './area-kinds';
import { BrowserStore } from './browser-store';

const EXAMPLES_KEY = 'area-examples';
const KINDS_KEY = 'area-kinds';
const LABELLED_KEY = 'area-labelled';
/** The examples KovOBS's own layout gives a recording until the user saves its areas (python/areas.py). */
const LAYOUT = 'kovobs:';

/** The files the review server keeps the area finder's training data in (test_out/vod_app/). */
export const EXAMPLES_FILE = 'area_examples.jsonl';
export const KINDS_FILE = 'area_kinds.json';

const isText = (v: unknown): v is string => typeof v === 'string';

function isExample(v: unknown): v is AreaExample {
  const e = v as AreaExample | null;
  return (
    typeof e === 'object' &&
    e !== null &&
    isText(e.rec) &&
    isText(e.kind) &&
    Array.isArray(e.feat) &&
    e.feat.every((x) => typeof x === 'number')
  );
}

function isKind(v: unknown): v is AreaKind {
  const k = v as AreaKind | null;
  return typeof k === 'object' && k !== null && isText(k.id) && isText(k.name);
}

/** JSON as Python's json.dumps writes it: every character outside printable ASCII as \uXXXX. */
function asciiJson(text: string): string {
  return text.replace(/[\u007f-￿]/g, (c) => `\\u${c.charCodeAt(0).toString(16).padStart(4, '0')}`);
}

/** A float as Python writes it: a whole number keeps its ".0". */
const pyFloat = (x: number) => (Number.isInteger(x) ? x.toFixed(1) : String(x));

/** An example as a line of area_examples.jsonl, written as python/areas.py writes it. */
export function exampleLine(e: AreaExample): string {
  const feat = e.feat.map(pyFloat).join(', ');
  return `{"rec": ${asciiJson(JSON.stringify(e.rec))}, "feat": [${feat}], "kind": ${asciiJson(JSON.stringify(e.kind))}}`;
}

/** Reads area_examples.jsonl: one example a line. Throws on a line that is not one. */
export function parseExamples(text: string): AreaExample[] {
  const out: AreaExample[] = [];
  for (const [i, line] of text.split(/\r?\n/).entries()) {
    if (!line.trim()) continue;
    let e: unknown;
    try {
      e = JSON.parse(line);
    } catch {
      e = null;
    }
    if (!isExample(e)) throw new Error(`line ${i + 1} is not an area example`);
    out.push({ rec: e.rec, feat: e.feat, kind: e.kind });
  }
  return out;
}

/** Reads area_kinds.json: a list of {id, name, about}. Throws when it is not one. */
export function parseKinds(text: string): AreaKind[] {
  const list: unknown = JSON.parse(text);
  if (!Array.isArray(list) || !list.every(isKind))
    throw new Error('it is not a list of area types with ids');
  return list.map((k: AreaKind) => ({
    id: k.id,
    name: k.name,
    about: isText(k.about) ? k.about : '',
  }));
}

/**
 * A recording opened in this browser as the examples name it, the way the review server names its recordings: a VOD
 * folder's video by its path below the folder ("Scenario/Scenario - 1 - stamp.mp4" when the folder is KovOBS's), a
 * file added from this computer as an upload ("uploads/name"). So the examples of both places can go in one file.
 */
export function exampleRec(id: string): string {
  if (id.startsWith('folder:')) return id.slice('folder:'.length);
  const added = /^local:\d+\/(.*)$/.exec(id);
  return added ? `uploads/${added[1]}` : id;
}

/**
 * The area finder's training data in this browser (IndexedDB), as the review server keeps it in test_out/vod_app/:
 * the examples saved areas make (area_examples.jsonl), the area types they name (area_kinds.json), and which
 * recordings have saved areas. Recordings are named as exampleRec names them. The user can load the server's files
 * into it and download it as the same files.
 */
@Injectable({ providedIn: 'root' })
export class AreaExamples {
  private readonly store = inject(BrowserStore);
  private readonly _examples = signal<readonly AreaExample[]>([]);
  private readonly _kinds = signal<readonly AreaKind[]>([]);
  private readonly saved = signal<ReadonlySet<string>>(new Set());
  readonly examples = this._examples.asReadonly();
  /** The area types loaded or added in this browser, in their order. */
  readonly kinds = this._kinds.asReadonly();
  /** The recordings with saved areas: those the user saved here, and those with examples of their own. */
  readonly labelled = computed<ReadonlySet<string>>(() => {
    const out = new Set(this.saved());
    for (const e of this._examples()) if (!e.rec.startsWith(LAYOUT)) out.add(e.rec);
    return out;
  });
  readonly count = computed<ExamplesCount>(() => ({
    examples: this._examples().length,
    recordings: this.labelled().size,
    kinds: this._kinds().length,
  }));
  /** The kept data has been read from the store. */
  readonly ready: Promise<void>;

  constructor() {
    this.ready = Promise.all([
      this.store.get<AreaExample[]>(EXAMPLES_KEY),
      this.store.get<AreaKind[]>(KINDS_KEY),
      this.store.get<string[]>(LABELLED_KEY),
    ])
      .then(([examples, kinds, saved]) => {
        if (examples) this._examples.set(examples);
        if (kinds) this._kinds.set(kinds);
        if (saved) this.saved.set(new Set(saved));
      })
      .catch(() => undefined);
  }

  /**
   * The user saved a recording's areas (its browser id), and the area finder learned these examples from them (the
   * core's learn): they replace the recording's earlier examples and its layout's, as python/areas.py `learn` does.
   */
  async learnt(id: string, examples: readonly AreaExample[]): Promise<void> {
    await this.ready;
    const rec = exampleRec(id);
    const keep = this._examples().filter((e) => e.rec !== rec && e.rec !== LAYOUT + rec);
    this._examples.set([...keep, ...examples.map((e) => ({ ...e, rec }))]);
    this.saved.update((s) => new Set([...s, rec]));
    await this.keep();
  }

  /** Keeps the area types (the areas editor adds and renames them). */
  async setKinds(kinds: readonly AreaKind[]): Promise<void> {
    await this.ready;
    this._kinds.set([...kinds]);
    await this.store.set(KINDS_KEY, this._kinds());
  }

  /**
   * Loads the review server's area_examples.jsonl and area_kinds.json (by their extensions, .jsonl and .json): the
   * examples of each recording in the file replace the browser's of that recording, and each area type replaces the
   * browser's with the same id (the others stay, after the file's).
   */
  async load(files: readonly File[]): Promise<ExamplesLoaded> {
    await this.ready;
    const out: ExamplesLoaded = { examples: 0, kinds: 0, refused: [] };
    for (const file of files) {
      try {
        if (/\.jsonl$/i.test(file.name)) {
          const loaded = parseExamples(await file.text());
          const recs = new Set(loaded.map((e) => e.rec));
          this._examples.update((now) => [...now.filter((e) => !recs.has(e.rec)), ...loaded]);
          out.examples += loaded.length;
        } else if (/\.json$/i.test(file.name)) {
          const loaded = parseKinds(await file.text());
          const ids = new Set(loaded.map((k) => k.id));
          this._kinds.update((now) => [...loaded, ...now.filter((k) => !ids.has(k.id))]);
          out.kinds += loaded.length;
        } else {
          out.refused.push(`${file.name} (not ${EXAMPLES_FILE} or ${KINDS_FILE})`);
        }
      } catch (e) {
        out.refused.push(`${file.name} (${e instanceof Error ? e.message : String(e)})`);
      }
    }
    await this.keep();
    return out;
  }

  /** The examples as area_examples.jsonl, written as the review server writes it. */
  examplesFile(): Blob {
    const text = this._examples()
      .map((e) => `${exampleLine(e)}\n`)
      .join('');
    return new Blob([text], { type: 'application/jsonl' });
  }

  /** The area types as area_kinds.json, written as the review server writes it: the built-in ones until there are any. */
  kindsFile(): Blob {
    const kinds = this._kinds().length ? this._kinds() : builtInKinds();
    return new Blob([asciiJson(JSON.stringify(kinds, null, 1))], {
      type: 'application/json',
    });
  }

  private async keep(): Promise<void> {
    await this.store.setMany([
      [EXAMPLES_KEY, this._examples()],
      [KINDS_KEY, this._kinds()],
      [LABELLED_KEY, [...this.saved()]],
    ]);
  }
}
