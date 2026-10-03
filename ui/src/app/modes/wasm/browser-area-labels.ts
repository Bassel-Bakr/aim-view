import { inject, Injectable, resource, ResourceRef } from '@angular/core';
import {
  AreaBox,
  AreaKind,
  AreaSet,
  FoundAreas,
  KeptAreas,
  KindEdit,
  RecordingAreas,
} from '../../api';
import { AreaLabels } from '../../platform/area-labels';
import { AreaExamples, exampleRec } from '../web-files/area-examples';
import {
  builtInKinds,
  editKinds,
  kovobsLayout,
  validAreas,
  withKindIds,
} from '../web-files/area-kinds';
import { LabelMarks } from '../web-files/label-marks';
import { LocalFile, LocalFiles } from '../web-files/local-files';
import { SavedAreas } from '../web-files/saved-areas';
import { BrowserAreaFinder } from './browser-area-finder';
import { CoreModule } from './core-module';

const NOT_OPEN = 'The recording is not open in this browser.';
const NOT_AREAS = 'boxes: a list of [x0, y0, x1, y1, type id] (shares of the frame)';

/** What a recording's areas are read from: the recording, and the kinds kept in this browser. */
interface AreasParams {
  id: string;
  kinds: readonly AreaKind[];
}

/**
 * The areas of the recordings opened in this browser, kept in it (SavedAreas), and the kinds, kept with the area
 * finder's examples (AreaExamples). A recording added from this computer without areas of its own starts from the ones
 * last saved for an added recording, as an upload does on the review server. The area finder (BrowserAreaFinder)
 * reads a recording's frames in a worker, once its review is done or when Find areas needs them, and keeps what it
 * found; Find areas and the learning work from that.
 */
@Injectable({ providedIn: 'root' })
export class BrowserAreaLabels implements AreaLabels {
  private readonly local = inject(LocalFiles);
  private readonly saved = inject(SavedAreas);
  private readonly examples = inject(AreaExamples);
  private readonly marks = inject(LabelMarks);
  private readonly finder = inject(BrowserAreaFinder);
  private readonly core = inject(CoreModule);
  readonly finderMissing = null;

  /** Read again when the kinds change (a kinds file loaded with the area finder's examples). */
  areas(id: () => string | undefined): ResourceRef<RecordingAreas | undefined> {
    return resource({
      params: (): AreasParams | undefined => {
        const at = id();
        return at === undefined ? undefined : { id: at, kinds: this.examples.kinds() };
      },
      loader: ({ params }) => this.read(params.id),
    });
  }

  layout(): Promise<AreaSet> {
    return Promise.resolve({ boxes: kovobsLayout(), source: 'kovobs' });
  }

  /**
   * From what the finder found in the recording (it reads the frames first when it has not yet, review or not): with
   * copy, the user's areas of a recording with the same layout (not one marked as another game); else the found areas,
   * named from the examples or by rules.
   */
  async find(id: string, copy: boolean): Promise<FoundAreas> {
    const f = this.open(id);
    const result = await this.finder.result(f.file);
    await Promise.all([this.examples.ready, this.marks.ready]);
    return this.core.areasFind({
      found: result.areas,
      examples: this.examples.examples(),
      labelled: copy ? await this.saved.labelled(f.file, this.marks.notAim()) : [],
      kinds: await this.kinds(),
    });
  }

  /**
   * Keeps the areas, and the finder learns from them what it found in the recording (once a reading under way ends;
   * nothing when it has not read the recording): they replace the recording's examples, and it counts as labelled.
   */
  async save(id: string, boxes: AreaBox[]): Promise<KeptAreas> {
    const f = this.open(id);
    if (!validAreas(boxes)) throw new Error(NOT_AREAS);
    const kinds = await this.kinds();
    const kept = withKindIds(boxes, kinds);
    const rec = exampleRec(id);
    const result = await this.finder.known(f.file);
    const name = f.file.name.replace(/\.\w+$/, '');
    const found = result && { rec, name, found: result.areas };
    await this.saved.save(f.file, kept, !this.local.lasting(id), found);
    const learnt = result
      ? await this.core.areasLearn({
          rec,
          found: result.areas,
          maps: result.maps,
          saved: kept,
          kinds,
        })
      : [];
    await this.examples.learnt(id, learnt);
    return { boxes: kept, source: 'saved' };
  }

  async saveKind(edit: KindEdit): Promise<AreaKind[]> {
    const kinds = editKinds(await this.kinds(), edit);
    await this.examples.setKinds(kinds);
    return kinds;
  }

  /** The areas the review of a recording opened here ignores. */
  async tracked(f: LocalFile): Promise<AreaBox[]> {
    return (await this.saved.areasOf(f.file, !this.local.lasting(f.id))).boxes;
  }

  private async read(id: string): Promise<RecordingAreas> {
    const f = this.open(id);
    const kinds = await this.kinds();
    const set = await this.saved.areasOf(f.file, !this.local.lasting(id));
    return { ...set, boxes: withKindIds(set.boxes, kinds), kinds };
  }

  /** The kinds kept in this browser, else the built-in ones. */
  private async kinds(): Promise<AreaKind[]> {
    await this.examples.ready;
    const kept = this.examples.kinds();
    return kept.length ? [...kept] : builtInKinds();
  }

  private open(id: string): LocalFile {
    const f = this.local.find(id);
    if (!f) throw new Error(NOT_OPEN);
    return f;
  }
}
