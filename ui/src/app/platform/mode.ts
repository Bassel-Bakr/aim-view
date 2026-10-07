/**
 * What a mode is (`Mode`): its name, the services it gives the contracts, and its HttpClient
 * features. Out: modes/mode.*.ts, each of which exports one; app.config.ts provides it.
 */

import { HttpFeature, HttpFeatureKind } from '@angular/common/http';
import { Provider } from '@angular/core';

/** The three ways the app runs: everything in the browser, with the review server, or as the desktop app. */
export type ModeName = 'browser' | 'server' | 'desktop';

/**
 * A way of running the app: the services it provides for the contracts in platform/ (RecordingSource, StatsFiles,
 * ReviewEngine, ModelCatalog and the others), and the HttpClient features it needs.
 */
export interface Mode {
  /** Which mode it is. */
  name: ModeName;
  /** Its class for each contract in platform/. */
  providers: Provider[];
  /** The HttpClient features it needs (the browser mode's interceptor, for one). */
  http: HttpFeature<HttpFeatureKind>[];
}
