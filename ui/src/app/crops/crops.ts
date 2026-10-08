/**
 * The Crops page (`Crops`, ?page=crops): picking a check folder and set, the crop on show with its
 * verdict buttons, and the answers' export and import. In: the CropDraft state. Out: the page;
 * the stage (crop-stage/) and the tools (crop-tools/) are its parts.
 */

import { Component, computed, inject, signal } from '@angular/core';
import { CropAnswers, CropPage, CropSet, CropVerdict } from '../api';
import { Button } from '../controls/button';
import { CropSets } from '../platform/crop-sets';
import { CropDraft, CropNote, messageOf } from './crop-draft';
import { CropStage } from './crop-stage/crop-stage';
import { CropTools } from './crop-tools/crop-tools';

/** What each verdict says on the page. */
const VERDICT_WORDS: Readonly<Record<CropVerdict, string>> = {
  right: 'Right',
  wrong: 'Wrong, fixed',
  unsure: "Can't tell",
};

/** Why a suggestion is offered. */
const SUGGESTION_WORDS = {
  preset: 'Suggested: the mined box starts crossed out.',
  lesson: 'Suggested from your fixes on this recording.',
  whole: 'Suggested from your fixes on this recording: one box per target.',
} as const;

/**
 * The Crops page: the detector's crops a miner picked (python/model/crop_check/make_page.py), one at a time, to say
 * whether the model found every target right, and to fix its shapes when not: pills and boxes, joined into targets,
 * in front of or behind each other, with head and body roles. The answers train the detector
 * (python/model/crop_check/labels.py). Phone first: the page is opened over the local network (?page=crops).
 */
@Component({
  selector: 'app-crops',
  imports: [Button, CropStage, CropTools],
  templateUrl: './crops.html',
  styleUrl: './crops.scss',
})
export class Crops {
  /** The page's state: the folder and set open, the crop on show, its answer and the fix. */
  protected readonly draft = inject(CropDraft);
  /** Whether the user can add a check folder from this computer (browser mode). */
  protected readonly canAddFolder = inject(CropSets).addFolder !== null;
  /** What an add, export or import is doing, or did; null before the first. */
  protected readonly working = signal<CropNote | null>(null);
  /** Every check folder; empty until they arrive. */
  protected readonly folders = computed<CropPage[]>(() =>
    this.draft.pages.hasValue() ? (this.draft.pages.value() ?? []) : [],
  );
  /** The open check folder's sets, one tab each. */
  protected readonly folderSets = computed<CropSet[]>(
    () => this.folders().find((page) => page.page === this.draft.folder())?.sets ?? [],
  );
  /** The share of the open set's crops answered, in percent. */
  protected readonly checkedShare = computed(() => {
    const count = this.draft.list().length;
    return count ? (100 * this.draft.answered()) / count : 0;
  });
  /** What the crop on show shows: the fix, the user's answer, a suggestion, or the model's shapes. */
  protected readonly status = computed(() => {
    if (this.draft.mode() === 'fix') return 'Fixing: Save keeps these shapes as the answer.';
    const answer = this.draft.answer();
    if (answer)
      return `Your answer: ${VERDICT_WORDS[answer.verdict]}${answer.suggested ? ' (as suggested)' : ''}.`;
    const suggestion = this.draft.suggestion();
    return suggestion
      ? SUGGESTION_WORDS[suggestion.why]
      : "The model's shapes. Is every target found, and only targets?";
  });

  /** Turns Skip checked on or off: after an answer, the next unchecked crop, or the next. */
  protected toggleSkipChecked(): void {
    this.draft.skipChecked.update((on) => !on);
  }

  /** Opens the check folder chosen in the list, at its first set with crops left to check. */
  protected openFolder(select: HTMLSelectElement): void {
    const page = this.folders().find((one) => one.page === select.value);
    const first = page?.sets.find((set) => set.answered < set.count) ?? page?.sets[0];
    if (page && first) this.draft.open(page.page, first.set);
  }

  /** Copies the check folder the user chose into this browser and opens it. */
  protected async addFolder(input: HTMLInputElement): Promise<void> {
    const files = [...(input.files ?? [])];
    input.value = '';
    if (!files.length) return;
    await this.run('Copying the folder into this browser', async () => {
      await this.draft.addFolder(files);
      return 'Folder added.';
    });
  }

  /** Saves every answer of the open folder as one file, for the review server's Import. */
  protected async exportAnswers(): Promise<void> {
    await this.run('Gathering the answers', async () => {
      const document = await this.draft.exportAnswers();
      if (!document) return 'No folder is open.';
      const url = URL.createObjectURL(
        new Blob([JSON.stringify(document)], { type: 'application/json' }),
      );
      const link = window.document.createElement('a');
      link.href = url;
      link.download = `${document.page}_answers.json`;
      link.click();
      setTimeout(() => URL.revokeObjectURL(url));
      return `Saved ${Object.keys(document.answers).length} answers.`;
    });
  }

  /** Imports a file of answers into the open check folder; a crop's newer answer wins. */
  protected async importAnswers(input: HTMLInputElement): Promise<void> {
    const [file] = input.files ?? [];
    input.value = '';
    if (!file) return;
    await this.run('Importing the answers', async () => {
      const done = await this.draft.importAnswers(JSON.parse(await file.text()) as CropAnswers);
      if (!done) return 'No folder is open.';
      const unknown = done.unknown ? `, ${done.unknown} of crops this folder does not have` : '';
      return `Imported: ${done.written} written, ${done.kept} kept (newer here)${unknown}.`;
    });
  }

  /** Runs a step, saying what it does, then what it did or why it failed. */
  private async run(doing: string, step: () => Promise<string>): Promise<void> {
    this.working.set({ text: `${doing}…`, failed: false });
    try {
      this.working.set({ text: await step(), failed: false });
    } catch (error) {
      this.working.set({ text: messageOf(error), failed: true });
    }
  }
}
