import { Mode } from '../platform/mode';
import { ModelCatalog } from '../platform/model-catalog';
import { RecordingSource } from '../platform/recording-source';
import { ReviewEngine } from '../platform/review-engine';
import { StatsFiles } from '../platform/stats-files';
import { BrowserModels } from './wasm/browser-models';
import { BrowserReview } from './wasm/browser-review';
import { LocalFiles } from './web-files/local-files';
import { LocalStatsFiles } from './web-files/local-stats-files';

/** Everything in the browser: files opened here, the review in WebAssembly, nothing sent anywhere. */
export const MODE: Mode = {
  name: 'browser',
  http: [],
  providers: [
    { provide: RecordingSource, useExisting: LocalFiles },
    { provide: StatsFiles, useExisting: LocalStatsFiles },
    { provide: ReviewEngine, useExisting: BrowserReview },
    { provide: ModelCatalog, useExisting: BrowserModels },
  ],
};
