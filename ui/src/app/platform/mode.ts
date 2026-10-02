import { HttpFeature, HttpFeatureKind } from '@angular/common/http';
import { Provider } from '@angular/core';

/** The three ways the app runs: everything in the browser, with the review server, or as the desktop app. */
export type ModeName = 'browser' | 'server' | 'desktop';

/**
 * A way of running the app: the services it provides for the contracts in platform/ (RecordingSource, StatsFiles,
 * ReviewEngine, ModelCatalog), and the HttpClient features it needs.
 */
export interface Mode {
  name: ModeName;
  providers: Provider[];
  http: HttpFeature<HttpFeatureKind>[];
}
