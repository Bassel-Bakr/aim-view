import { Component, resource } from '@angular/core';
import { getJson, Models } from './api';

@Component({
  selector: 'app-root',
  templateUrl: './app.html',
  styleUrl: './app.css',
})
export class App {
  protected readonly models = resource({
    loader: ({ abortSignal }) => getJson<Models>('/api/models', abortSignal),
  });
}
