import { Component, inject } from '@angular/core';
import { CutoffMenu } from './cutoff-menu/cutoff-menu';
import { LabelMenu } from './labelling/label-menu/label-menu';
import { ModelPanel } from './model-panel/model-panel';
import { MouseSwitch } from './mouse-switch/mouse-switch';
import { Recordings } from './recordings/recordings';
import { Run } from './run/run';
import { Library } from './services/library';
import { Upload } from './upload/upload';

@Component({
  selector: 'app-root',
  imports: [Upload, ModelPanel, LabelMenu, CutoffMenu, MouseSwitch, Recordings, Run],
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App {
  protected readonly library = inject(Library);
}
