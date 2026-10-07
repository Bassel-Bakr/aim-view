/**
 * The page's entry point: starts the app's root component (`App`) with its providers
 * (app.config.ts, which carry the build's mode). A failure to start goes to the console.
 */

import { bootstrapApplication } from '@angular/platform-browser';
import { appConfig } from './app/app.config';
import { App } from './app/app';

bootstrapApplication(App, appConfig).catch((err) => console.error(err));
