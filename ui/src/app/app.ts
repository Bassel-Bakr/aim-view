import { Component, inject, resource } from '@angular/core';
import { getJson, Models } from './api';
import { Library } from './library';
import { Recordings } from './recordings/recordings';
import { RunHeader } from './run/run-header';

@Component({
  selector: 'app-root',
  imports: [Recordings, RunHeader],
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App {
  protected readonly library = inject(Library);
  protected readonly models = resource({
    loader: ({ abortSignal }) => getJson<Models>('/api/models', abortSignal),
  });
}
