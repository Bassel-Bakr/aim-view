/**
 * The app's root component (`App`, `<app-root>`): the top bar and either the review page (the
 * recordings list and the run) or the Crops page. In: the open recording (Library) and the page
 * state (Pages). Out: app.html.
 */

import { Component, inject } from '@angular/core';
import { Crops } from './crops/crops';
import { CutoffMenu } from './cutoff-menu/cutoff-menu';
import { LabelMenu } from './labelling/label-menu/label-menu';
import { ModelPanel } from './model-panel/model-panel';
import { MouseSwitch } from './mouse-switch/mouse-switch';
import { Recordings } from './recordings/recordings';
import { Run } from './run/run';
import { Library } from './services/library';
import { Pages } from './services/pages';
import { Upload } from './upload/upload';

/**
 * The whole page: the top bar's tools, then the review page or the Crops page (loaded when first
 * opened). The host's data-page and data-list attributes lay it out.
 */
@Component({
  selector: 'app-root',
  imports: [Upload, ModelPanel, LabelMenu, CutoffMenu, MouseSwitch, Recordings, Run, Crops],
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
}
