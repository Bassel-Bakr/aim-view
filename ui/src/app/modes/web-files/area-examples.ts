import { computed, inject, Injectable, signal } from '@angular/core';
import { AreaExample, AreaKind } from '../../api';
import { ExamplesCount, ExamplesLoaded } from '../../platform/labelling';
import { builtInKinds } from './area-kinds';
import { BrowserStore } from './browser-store';
import { BundledData, EXAMPLES_FILE, KINDS_FILE } from './bundled-data';

const EXAMPLES_KEY = 'area-examples';
const KINDS_KEY = 'area-kinds';
const LABELLED_KEY = 'area-labelled';
/** The examples KovOBS's own layout gives a recording until the user saves its areas (python/areas.py). */
const LAYOUT = 'kovobs:';

export { EXAMPLES_FILE, KINDS_FILE };

/** The recording an example is of: its own, or the one whose layout it is. */
const recordingOf = (rec: string) => (rec.startsWith(LAYOUT) ? rec.slice(LAYOUT.length) : rec);

/** A review server's file read by parse; none when there is no file, or it cannot be read. */
function parsedOr<T>(parse: (text: string) => T[], text: string | null): T[] {
  if (text === null) return [];
  try {
    return parse(text);
  } catch {
    return [];
  }
}

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
 * into it and download it as the same files. The review server's files shipped with the app (BundledData) are the
 * start: a recording's examples learnt or loaded here replace its examples there, and an area type changed or loaded
 * here replaces the one with its id. Only what is learnt, loaded or changed here is kept in the browser.
 */
@Injectable({ providedIn: 'root' })
export class AreaExamples {
  private readonly store = inject(BrowserStore);
  private readonly bundled = inject(BundledData);
  /** The examples learnt or loaded in this browser. */
  private readonly own = signal<readonly AreaExample[]>([]);
  /** The area types added, changed or loaded in this browser, in their order. */
  private readonly ownKinds = signal<readonly AreaKind[]>([]);
  private readonly saved = signal<ReadonlySet<string>>(new Set());
  private readonly serverExamples = computed(() =>
    parsedOr(parseExamples, this.bundled.data().examples),
  );
  private readonly serverKinds = computed(() => parsedOr(parseKinds, this.bundled.data().kinds));
  /** The review server's examples but those of the recordings learnt or loaded here, then this browser's. */
  readonly examples = computed<readonly AreaExample[]>(() => {
    const own = this.own();
    const here = new Set([...this.saved(), ...own.map((e) => recordingOf(e.rec))]);
    return [...this.serverExamples().filter((e) => !here.has(recordingOf(e.rec))), ...own];
  });
  /** The area types: the review server's (each as changed here), then those added here, in their order. */
  readonly kinds = computed<readonly AreaKind[]>(() => {
    const own = this.ownKinds();
    const server = this.serverKinds();
    const changed = new Map(own.map((k) => [k.id, k]));
    const ids = new Set(server.map((k) => k.id));
    return [...server.map((k) => changed.get(k.id) ?? k), ...own.filter((k) => !ids.has(k.id))];
  });
  /** The recordings with saved areas: those the user saved here, and those with examples of their own. */
  readonly labelled = computed<ReadonlySet<string>>(() => {
    const out = new Set(this.saved());
    for (const e of this.examples()) if (!e.rec.startsWith(LAYOUT)) out.add(e.rec);
    return out;
  });
  readonly count = computed<ExamplesCount>(() => ({
    examples: this.examples().length,
    recordings: this.labelled().size,
    kinds: this.kinds().length,
  }));
  /** The kept data has been read from the store, and the review server's files shipped with the app. */
  readonly ready: Promise<void>;

  constructor() {
    this.ready = Promise.all([
      this.store.get<AreaExample[]>(EXAMPLES_KEY),
      this.store.get<AreaKind[]>(KINDS_KEY),
      this.store.get<string[]>(LABELLED_KEY),
      this.bundled.load(),
    ])
      .then(([examples, kinds, saved]) => {
        if (examples) this.own.set(examples);
        if (kinds) this.ownKinds.set(kinds);
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
    const keep = this.own().filter((e) => e.rec !== rec && e.rec !== LAYOUT + rec);
    this.own.set([...keep, ...examples.map((e) => ({ ...e, rec }))]);
    this.saved.update((s) => new Set([...s, rec]));
    await this.keep();
  }

  /** Keeps the area types (the areas editor adds and renames them). */
  async setKinds(kinds: readonly AreaKind[]): Promise<void> {
    await this.ready;
    this.ownKinds.set([...kinds]);
    await this.store.set(KINDS_KEY, this.ownKinds());
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
          this.own.update((now) => [...now.filter((e) => !recs.has(e.rec)), ...loaded]);
          out.examples += loaded.length;
        } else if (/\.json$/i.test(file.name)) {
          const loaded = parseKinds(await file.text());
          const ids = new Set(loaded.map((k) => k.id));
          this.ownKinds.update((now) => [...loaded, ...now.filter((k) => !ids.has(k.id))]);
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
    const text = this.examples()
      .map((e) => `${exampleLine(e)}\n`)
      .join('');
    return new Blob([text], { type: 'application/jsonl' });
  }

  /** The area types as area_kinds.json, written as the review server writes it: the built-in ones until there are any. */
  kindsFile(): Blob {
    const kinds = this.kinds().length ? this.kinds() : builtInKinds();
    return new Blob([asciiJson(JSON.stringify(kinds, null, 1))], {
      type: 'application/json',
    });
  }

  private async keep(): Promise<void> {
    await this.store.setMany([
      [EXAMPLES_KEY, this.own()],
      [KINDS_KEY, this.ownKinds()],
      [LABELLED_KEY, [...this.saved()]],
    ]);
  }
}
