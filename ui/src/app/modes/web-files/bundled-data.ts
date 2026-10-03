import { HttpClient } from '@angular/common/http';
import { inject, Injectable, signal } from '@angular/core';
import { firstValueFrom } from 'rxjs';

/** Where `bun run assets` puts the user's data beside the app (ui/generated/data/). A build for others has none. */
const FOLDER = 'data/';
/** The files the review server keeps the area finder's training data in (test_out/vod_app/). */
export const EXAMPLES_FILE = 'area_examples.jsonl';
export const KINDS_FILE = 'area_kinds.json';

/** The review server's area finder files as text, as it keeps them; null for a file there is none of. */
export interface Bundle {
  examples: string | null;
  kinds: string | null;
}

const NONE: Bundle = { examples: null, kinds: null };

/**
 * The area finder's training data from the review server's folder (test_out/vod_app/: area_examples.jsonl and
 * area_kinds.json), shipped beside the app by `bun run assets`, which browser mode starts from (AreaExamples). Read
 * once, when first asked for; a build without it, or a file that cannot be read, gives none. Without HttpClient (the
 * browser mode's tests) there is none.
 */
@Injectable({ providedIn: 'root' })
export class BundledData {
  private readonly http = inject(HttpClient, { optional: true });
  private reading: Promise<Bundle> | null = null;
  private readonly _data = signal<Bundle>(NONE);
  /** The files once read; none until then. */
  readonly data = this._data.asReadonly();

  /** Reads the files (once), and gives them. */
  load(): Promise<Bundle> {
    this.reading ??= this.read().then((data) => {
      this._data.set(data);
      return data;
    });
    return this.reading;
  }

  private async read(): Promise<Bundle> {
    const http = this.http;
    if (!http) return NONE;
    const text = (file: string) =>
      firstValueFrom(http.get(FOLDER + file, { responseType: 'text' })).catch(() => null);
    const [examples, kinds] = await Promise.all([text(EXAMPLES_FILE), text(KINDS_FILE)]);
    return { examples, kinds };
  }
}
