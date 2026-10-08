/**
 * The app's root component (`App`, `<app-root>`): the top bar and either the review page (the
 * recordings list and the run) or the Crops page. In: the open recording (Library) and the page
 * state (Pages). Out: app.html, and the top bar's More menu placed under its button.
 */

import { NgTemplateOutlet } from '@angular/common';
import { Component, effect, ElementRef, inject, untracked, viewChild } from '@angular/core';
import { Crops } from './crops/crops';
import { CutoffMenu } from './cutoff-menu/cutoff-menu';
import { LabelMenu } from './labelling/label-menu/label-menu';
import { ModelPanel } from './model-panel/model-panel';
import { MouseSwitch } from './mouse-switch/mouse-switch';
import { Recordings } from './recordings/recordings';
import { Run } from './run/run';
import { StoragePanel } from './storage-panel/storage-panel';
import { Library } from './services/library';
import { Pages } from './services/pages';
import { FolderPicks } from './upload/folder-picks/folder-picks';
import { LinkForm } from './upload/link-form/link-form';
import { Upload } from './upload/upload';

/** The gap between the More button and its menu, in px. */
const MENU_GAP_PX = 4;

/**
 * The whole page: the top bar's tools, then the review page or the Crops page (loaded when first
 * opened). The host's data-page and data-list attributes lay it out. Where the top bar has no room
 * for every tool (Pages.toolsMenu), the less-used ones are in a More menu, which closes when a
 * recording opens (a queue's next one).
 */
@Component({
  selector: 'app-root',
  imports: [
    NgTemplateOutlet,
    Upload,
    FolderPicks,
    LinkForm,
    ModelPanel,
    LabelMenu,
    CutoffMenu,
    MouseSwitch,
    StoragePanel,
    Recordings,
    Run,
    Crops,
  ],
  templateUrl: './app.html',
  styleUrl: './app.scss',
  host: {
    '[attr.data-page]': "pages.crops() ? 'crops' : 'review'",
    '[attr.data-list]': "pages.listOpen() ? 'open' : 'closed'",
  },
})
export class App {
  /** The recordings and which one is open, for the template. */
  protected readonly library = inject(Library);
  /** Which page shows and whether the recordings list is open. */
  protected readonly pages = inject(Pages);
  /** The top bar's More menu; there only while the bar has no room for every tool. */
  private readonly toolsMenu = viewChild<ElementRef<HTMLElement>>('toolsMenu');

  /** Closes the More menu when a recording opens, so it does not cover the page a queue opens. */
  constructor() {
    effect(() => {
      this.library.selectedId();
      untracked(() => this.toolsMenu()?.nativeElement.hidePopover());
    });
  }

  /**
   * Places the More menu under its button, its left edge on the button's but never past the page's right edge, in px.
   * It runs before the menu shows, so the menu's width is its style's (--top-bar-menu-width), not a measured box.
   */
  protected placeMenu(button: HTMLElement, menu: HTMLElement): void {
    const box = button.getBoundingClientRect();
    const roomPx = document.documentElement.clientWidth - parseFloat(getComputedStyle(menu).width);
    menu.style.top = `${box.bottom + MENU_GAP_PX}px`;
    menu.style.left = `${Math.max(0, Math.min(box.left, roomPx))}px`;
  }
}
