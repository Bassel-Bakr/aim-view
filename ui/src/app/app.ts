import { Component, inject } from '@angular/core';
import { Recordings } from './recordings/recordings';
import { Run } from './run/run';
import { Library } from './services/library';
import { Models } from './services/models';
import { pill } from '@themes/controls.styles';
import { appStyles } from '@themes/app.styles';
import { slotClasses } from '@themes/slot-classes';

@Component({
  selector: 'app-root',
  imports: [Recordings, Run],
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App {
  protected readonly library = inject(Library);
  protected readonly models = inject(Models).list;
  protected readonly ui = slotClasses(appStyles());
  protected readonly pill = pill();
}
