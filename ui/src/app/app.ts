import { Component, inject } from '@angular/core';
import { Recordings } from './recordings/recordings';
import { Run } from './run/run';
import { Library } from './services/library';
import { Models } from './services/models';

@Component({
  selector: 'app-root',
  imports: [Recordings, Run],
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App {
  protected readonly library = inject(Library);
  protected readonly models = inject(Models).list;
}
