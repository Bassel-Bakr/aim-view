import { HttpClient } from '@angular/common/http';
import { computed, inject, Injectable, signal } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { AreaExample, AreaKind } from '../../api';
import { ExamplesCount, ExamplesLoaded, ExamplesStore } from '../../platform/labelling';
import { ServerLabelling } from '../http/server-labelling';
import { builtInKinds } from '../web-files/area-kinds';
import { MountedFiles } from './mounted-files';

/** The area finder's files, as the review service keeps them in its data folder (and the review server in vod_app/). */
const EXAMPLES_FILE = 'area_examples.jsonl';
const KINDS_FILE = 'area_kinds.json';
/** Where the service keeps the area types (its data folder): the page reads and writes the file itself. */
const KINDS = `/data/${KINDS_FILE}`;
/** The examples KovOBS's own layout gives a recording until the user saves its areas (python/areas.py). */
const LAYOUT = 'kovobs:';

/** The two files' text as kept (null: there is none). */
interface FinderTexts {
  examples: string | null;
  kinds: string | null;
}

/** An example's line of area_examples.jsonl: the example, and the line as written. */
interface ExampleLine {
  example: AreaExample;
  line: string;
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

/** Reads area_examples.jsonl: one example a line, kept as written. Throws on a line that is not one. */
function parseExamples(text: string): ExampleLine[] {
  const out: ExampleLine[] = [];
  for (const [i, line] of text.split(/\r?\n/).entries()) {
    if (!line.trim()) continue;
    let e: unknown;
    try {
      e = JSON.parse(line);
    } catch {
      e = null;
    }
    if (!isExample(e)) throw new Error(`line ${i + 1} is not an area example`);
    out.push({ example: e, line });
  }
  return out;
}

/** Reads area_kinds.json: a list of {id, name, about}. Throws when it is not one. */
function parseKinds(text: string): AreaKind[] {
  const list: unknown = JSON.parse(text);
  if (!Array.isArray(list) || !list.every(isKind))
    throw new Error('it is not a list of area types with ids');
  return list.map((k: AreaKind) => ({
    id: k.id,
    name: k.name,
    about: isText(k.about) ? k.about : '',
  }));
}

/** A parse of a file kept here; nothing when it cannot be read. */
function parsedOr<T>(parse: (text: string) => T[], text: string | null): T[] {
  if (text === null) return [];
  try {
    return parse(text);
  } catch {
    return [];
  }
}

/** JSON as Python's json.dumps writes it: every character outside printable ASCII as \uXXXX. */
function asciiJson(text: string): string {
  return text.replace(/[\u007f-￿]/g, (c) => `\\u${c.charCodeAt(0).toString(16).padStart(4, '0')}`);
}

/**
 * The area finder's training data in browser mode: the review service's area_examples.jsonl (/api/area_examples) and
 * area_kinds.json in its data folder in this browser, which the service learns into when areas are saved. The user
 * downloads them as they are, and loads the review server's files into them: the examples of each recording in a file
 * replace that recording's, and each area type replaces the one with its id (the others stay, after the file's).
 */
@Injectable({ providedIn: 'root' })
export class BrowserExamples implements ExamplesStore {
  private readonly http = inject(HttpClient);
  private readonly files = inject(MountedFiles);
  private readonly version = signal(0);
  /** Counts up when the files change here (areas saved, a kind edited, files loaded): what shows them reads again. */
  readonly changes = this.version.asReadonly();
  /** The two files as last read. */
  private readonly texts = signal<FinderTexts>({ examples: null, kinds: null });
  /** The reading asked for last: an older one that ends later is dropped. */
  private reading = 0;
  readonly fileNames = [EXAMPLES_FILE, KINDS_FILE];
  readonly count = computed<ExamplesCount>(() => {
    const t = this.texts();
    const examples = parsedOr(parseExamples, t.examples).map((e) => e.example);
    const kinds = t.kinds === null ? builtInKinds() : parsedOr(parseKinds, t.kinds);
    const recordings = new Set(examples.filter((e) => !e.rec.startsWith(LAYOUT)).map((e) => e.rec));
    return { examples: examples.length, recordings: recordings.size, kinds: kinds.length };
  });

  constructor() {
    void this.refresh();
  }

  /** The files changed: they are read again. */
  changed(): Promise<void> {
    this.version.update((v) => v + 1);
    return this.refresh();
  }

  private async refresh(): Promise<void> {
    const ticket = ++this.reading;
    const [examples, kinds] = await Promise.all([this.examplesText(), this.files.text(KINDS)]);
    if (ticket === this.reading) this.texts.set({ examples, kinds });
  }

  /** The service's area_examples.jsonl ('' when there is none); null when it cannot be read. */
  private examplesText(): Promise<string | null> {
    const asked = this.http.get('/api/area_examples', { responseType: 'text' });
    return firstValueFrom(asked).catch(() => null);
  }

  /** One of the two files as kept; the built-in area types until there are any. */
  async file(name: string): Promise<Blob> {
    if (name === KINDS_FILE) {
      const text = await this.files.text(KINDS);
      return new Blob([text ?? asciiJson(JSON.stringify(builtInKinds(), null, 1))], {
        type: 'application/json',
      });
    }
    return new Blob([(await this.examplesText()) ?? ''], { type: 'application/jsonl' });
  }

  /** Loads area_examples.jsonl and area_kinds.json (by their extensions, .jsonl and .json) into the kept ones. */
  async load(files: readonly File[]): Promise<ExamplesLoaded> {
    const out: ExamplesLoaded = { examples: 0, kinds: 0, refused: [] };
    for (const file of files) {
      try {
        if (/\.jsonl$/i.test(file.name)) out.examples += await this.loadExamples(file);
        else if (/\.json$/i.test(file.name)) out.kinds += await this.loadKinds(file);
        else out.refused.push(`${file.name} (not ${EXAMPLES_FILE} or ${KINDS_FILE})`);
      } catch (e) {
        out.refused.push(`${file.name} (${e instanceof Error ? e.message : String(e)})`);
      }
    }
    await this.changed();
    return out;
  }

  /** The file's examples in place of the kept ones of the same recordings; how many it held. */
  private async loadExamples(file: File): Promise<number> {
    const loaded = parseExamples(await file.text());
    const recs = new Set(loaded.map((e) => e.example.rec));
    const now = await this.examplesText();
    if (now === null) throw new Error('the examples kept here could not be read');
    const kept = parseExamples(now);
    const lines = [...kept.filter((e) => !recs.has(e.example.rec)), ...loaded].map(
      (e) => `${e.line}\n`,
    );
    await firstValueFrom(this.http.post('/api/area_examples', lines.join('')));
    return loaded.length;
  }

  /** The file's area types first, then the kept ones it does not hold; how many it held. */
  private async loadKinds(file: File): Promise<number> {
    const loaded = parseKinds(await file.text());
    const ids = new Set(loaded.map((k) => k.id));
    const text = await this.files.text(KINDS);
    const kept = text === null ? builtInKinds() : parsedOr(parseKinds, text);
    const kinds = [...loaded, ...kept.filter((k) => !ids.has(k.id))];
    await this.files.write(KINDS, asciiJson(JSON.stringify(kinds, null, 1)));
    return loaded.length;
  }
}

/**
 * Labelling in browser mode: the review service's queue, skips and other games, as on the review server, and the area
 * finder's training data kept in this browser, to download and to load (BrowserExamples).
 */
@Injectable({ providedIn: 'root' })
export class BrowserLabelling extends ServerLabelling {
  override readonly examples = inject(BrowserExamples);
}
