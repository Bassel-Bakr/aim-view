import { computed, effect, inject, resource, Service, signal, untracked } from '@angular/core';
import {
  CropAnswer,
  CropAnswers,
  CropEntry,
  CropSet,
  CropVerdict,
  SceneView,
  ShapeKind,
} from '../api';
import { CoreModule, ShapesRequest } from '../modes/wasm/core-module';
import { CropAnswerMap, CropImport, CropSets } from '../platform/crop-sets';
import { queryValue, setQuery } from '../services/url-query';
import { learn, suggest, Suggestion } from './crop-lessons';
import { answerOf, DraftScene, duplicated, sceneOfAnswer, sceneOfFix } from './crop-scene';

/** A crop's side, in pixels (make_page.py's crops). */
export const CROP_SIDE = 256;

/** How far a copy lands from the shape it copies, down and to the right (crop pixels). */
const COPY_SHIFT_PX = 6;

/** The page shows a crop's marks, or edits them. */
export type CropMode = 'view' | 'fix';

/** What the page last said: a status, or something that failed. */
export interface CropNote {
  text: string;
  failed: boolean;
}

/**
 * The Crops page's state: the check folder and set open (kept in the URL), their crops and answers, the crop on show
 * and its scene (its answer's, the suggestion's, or the one being fixed), the shapes selected, what the core says the
 * scene shows, and saving an answer.
 */
@Service()
export class CropDraft {
  private readonly sets = inject(CropSets);
  private readonly core = inject(CoreModule);
  readonly folder = signal<string | undefined>(queryValue('folder') ?? undefined);
  readonly set = signal<string | undefined>(queryValue('set') ?? undefined);
  readonly pages = this.sets.pages();
  readonly crops = this.sets.crops(this.folder, this.set);
  private readonly loaded = this.sets.answers(this.folder, this.set);
  private readonly saved = signal<CropAnswerMap>({});
  readonly answers = computed<CropAnswerMap>(() => ({
    ...(this.loaded.hasValue() ? this.loaded.value() : {}),
    ...this.saved(),
  }));
  readonly index = signal(0);
  readonly mode = signal<CropMode>('view');
  readonly draft = signal<DraftScene | null>(null);
  readonly selection = signal<string[]>([]);
  /** The kind of shape a drag on the wall draws. */
  readonly kind = signal<ShapeKind>('pill');
  /** In a fix, a drag moves the view instead of drawing or moving shapes (the tools' Pan); taps still select. */
  readonly panning = signal(false);
  readonly note = signal<CropNote | null>(null);
  readonly busy = signal(false);
  readonly list = computed<CropEntry[]>(() =>
    this.crops.hasValue() ? (this.crops.value() ?? []) : [],
  );
  readonly crop = computed<CropEntry | null>(() => this.list()[this.index()] ?? null);
  readonly setInfo = computed<CropSet | null>(() => {
    const page = (this.pages.hasValue() ? this.pages.value() : [])?.find(
      (one) => one.page === this.folder(),
    );
    return page?.sets.find((one) => one.set === this.set()) ?? null;
  });
  readonly answered = computed(
    () => this.list().filter((crop) => crop.id in this.answers()).length,
  );
  readonly answer = computed<CropAnswer | null>(() => {
    const crop = this.crop();
    return crop ? (this.answers()[crop.id] ?? null) : null;
  });
  private readonly lessons = computed(() => learn(this.list(), this.answers()));
  readonly suggestion = computed<Suggestion | null>(() => {
    const crop = this.crop();
    if (!crop || this.answer()) return null;
    return suggest(crop, this.lessons().get(crop.folder) ?? null, this.setInfo()?.learn ?? true);
  });
  /** The scene on show: the one being fixed, the answer's, or the crop's boxes with the suggestion applied. */
  readonly shown = computed<DraftScene | null>(() => {
    const crop = this.crop();
    if (!crop) return null;
    if (this.mode() === 'fix') return this.draft();
    const answer = this.answer();
    return answer ? sceneOfAnswer(crop, answer) : sceneOfFix(crop, this.suggestion()?.fix ?? null);
  });
  /** What the core says the scene shows: each target's visible pixels and box. */
  readonly view = resource<SceneView, ShapesRequest | undefined>({
    params: () => {
      const scene = this.shown();
      if (!scene) return undefined;
      const { shapes, targets, occluders } = scene;
      return { scene: { shapes, targets, occluders }, width: CROP_SIDE, height: CROP_SIDE };
    },
    loader: ({ params }) => this.core.shapesVisible(params),
  });

  constructor() {
    effect(() => setQuery({ folder: this.folder() ?? null, set: this.set() ?? null }));
    effect(() => this.openFirst());
    // which crops are checked is known only once the answers are in: the crops often come first
    effect(() => {
      const list = this.list();
      if (!this.loaded.hasValue() && !this.loaded.error()) return;
      untracked(() => this.toFirstUnchecked(list));
    });
  }

  /** With nothing open, the first check folder with crops left to check opens, at that set. */
  private openFirst(): void {
    if (!this.pages.hasValue() || this.folder()) return;
    const pages = this.pages.value() ?? [];
    const open = pages.flatMap((page) => page.sets.map((set) => ({ page: page.page, set })));
    const first = open.find(({ set }) => set.answered < set.count) ?? open[0];
    if (!first) return;
    this.folder.set(first.page);
    this.set.set(first.set.set);
  }

  /** A set's crops arrived: the first not yet checked shows. */
  private toFirstUnchecked(list: readonly CropEntry[]): void {
    const first = list.findIndex((crop) => !(crop.id in this.answers()));
    this.index.set(first >= 0 ? first : 0);
    this.mode.set('view');
  }

  /** Opens a check folder's set. */
  open(folder: string, set: string): void {
    this.folder.set(folder);
    this.set.set(set);
    this.saved.set({});
  }

  /** Moves through the crops (keeps within them). */
  go(step: number): void {
    this.cancel();
    this.index.set(Math.min(Math.max(this.index() + step, 0), Math.max(this.list().length - 1, 0)));
  }

  /** The next crop not yet checked, after this one (round to the start); past the last when all are checked. */
  nextUnchecked(): void {
    this.cancel();
    const list = this.list();
    for (let i = 1; i <= list.length; i++) {
      const at = (this.index() + i) % list.length;
      if (!(list[at].id in this.answers())) return this.index.set(at);
    }
    this.index.set(list.length);
  }

  /** Starts fixing the crop on show, from what it shows. */
  fix(): void {
    const shown = this.shown();
    if (!shown) return;
    this.draft.set(shown);
    this.selection.set([]);
    this.mode.set('fix');
  }

  /** Leaves the fix unsaved. */
  cancel(): void {
    this.mode.set('view');
    this.draft.set(null);
    this.selection.set([]);
  }

  /** Changes the scene being fixed. */
  edit(change: (scene: DraftScene) => DraftScene): void {
    const draft = this.draft();
    if (draft) this.draft.set(change(draft));
  }

  /** The selected shapes copied a little down and to the right; the copies are selected, to drag into place. */
  duplicate(): void {
    const [draft, ids] = [this.draft(), this.selection()];
    if (!draft || !ids.length) return;
    const copy = duplicated(draft, ids, [COPY_SHIFT_PX, COPY_SHIFT_PX]);
    this.draft.set(copy.scene);
    this.selection.set(copy.copies);
  }

  /** Right or Can't tell, for what the crop shows (a suggestion taken as offered says so). */
  judge(verdict: Exclude<CropVerdict, 'wrong'>): Promise<void> {
    const [crop, shown] = [this.crop(), this.shown()];
    if (!crop || !shown) return Promise.resolve();
    const offered = verdict === 'right' && !this.answer() && this.suggestion() !== null;
    return this.keep(answerOf(crop, shown, verdict, offered));
  }

  /** Saves the fix as the crop's answer (Wrong, with the scene drawn). */
  saveFix(): Promise<void> {
    const [crop, draft] = [this.crop(), this.draft()];
    if (!crop || !draft) return Promise.resolve();
    return this.keep(answerOf(crop, draft, 'wrong', false));
  }

  /** Keeps an answer, then shows the next crop not yet checked; a failure says why and stays. */
  private async keep(answer: CropAnswer): Promise<void> {
    const [crop, folder] = [this.crop(), this.folder()];
    if (!crop || !folder) return;
    this.busy.set(true);
    try {
      const kept = await this.sets.save(folder, crop.id, answer);
      this.saved.update((saved) => ({ ...saved, [crop.id]: kept }));
      this.note.set(null);
      this.nextUnchecked();
    } catch (error) {
      this.note.set({ text: `That answer was not saved: ${messageOf(error)}`, failed: true });
    } finally {
      this.busy.set(false);
    }
  }

  /** Every answer of the open check folder, as the document an import takes. */
  exportAnswers(): Promise<CropAnswers | null> {
    const folder = this.folder();
    return folder ? this.sets.exportAnswers(folder) : Promise.resolve(null);
  }

  /** Imports a document of answers into the open check folder, and says what it did. */
  async importAnswers(document: CropAnswers): Promise<CropImport | null> {
    const folder = this.folder();
    if (!folder) return null;
    const done = await this.sets.importAnswers(folder, document);
    this.loaded.reload();
    this.pages.reload();
    this.saved.set({});
    return done;
  }

  /** Adds a check folder the user chose (browser mode), and opens it. */
  async addFolder(files: readonly File[]): Promise<void> {
    const add = this.sets.addFolder;
    if (!add) return;
    const name = await add(files);
    this.pages.reload();
    this.folder.set(name);
    this.set.set(undefined);
  }
}

/** Something with an error in it: an HttpErrorResponse, or the service's answer to a refused request. */
interface WithError {
  error: unknown;
}

/** An error's message, for a note: the service's words when it refused, else the error's own. */
export function messageOf(error: unknown): string {
  if (error && typeof error === 'object' && 'error' in error) {
    const body = (error as WithError).error;
    if (body && typeof body === 'object' && 'error' in body)
      return String((body as WithError).error);
  }
  return error instanceof Error ? error.message : String(error);
}
