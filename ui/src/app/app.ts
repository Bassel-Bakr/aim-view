import { Component, inject } from '@angular/core';
import { ModelPanel } from './model-panel/model-panel';
import { Recordings } from './recordings/recordings';
import { Run } from './run/run';
import { Library } from './services/library';
import { Upload } from './upload/upload';
import { appStyles } from '@themes/app.styles';
import { slotClasses } from '@themes/slot-classes';

@Component({
  selector: 'app-root',
  imports: [Upload, ModelPanel, Recordings, Run],
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App {
  protected readonly library = inject(Library);
  protected readonly ui = slotClasses(appStyles());
}
