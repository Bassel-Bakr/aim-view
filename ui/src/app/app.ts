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

@Component({
  selector: 'app-root',
  imports: [Upload, ModelPanel, LabelMenu, CutoffMenu, MouseSwitch, Recordings, Run, Crops],
  templateUrl: './app.html',
  styleUrl: './app.scss',
  host: { '[attr.data-page]': "pages.crops() ? 'crops' : 'review'" },
})
export class App {
  protected readonly library = inject(Library);
  protected readonly pages = inject(Pages);
}
