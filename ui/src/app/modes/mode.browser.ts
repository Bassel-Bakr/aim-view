import { AreaLabels } from '../platform/area-labels';
import { FaintCutoffs } from '../platform/faint-cutoffs';
import { Mode } from '../platform/mode';
import { Labelling } from '../platform/labelling';
import { ModelCatalog } from '../platform/model-catalog';
import { MouseLogs } from '../platform/mouse-logs';
import { RecordingSource } from '../platform/recording-source';
import { ReviewEngine } from '../platform/review-engine';
import { ScoreHistory } from '../platform/score-history';
import { StatsFiles } from '../platform/stats-files';
import { BrowserAreaLabels } from './wasm/browser-area-labels';
import { BrowserFaintCutoffs } from './wasm/browser-faint-cutoffs';
import { BrowserModels } from './wasm/browser-models';
import { BrowserMouseLogs } from './wasm/browser-mouse-logs';
import { BrowserReview } from './wasm/browser-review';
import { BrowserLabelling } from './web-files/browser-labelling';
import { LocalFiles } from './web-files/local-files';
import { LocalScoreHistory } from './web-files/local-score-history';
import { LocalStatsFiles } from './web-files/local-stats-files';

/** Everything in the browser: files opened here, the review in WebAssembly, nothing sent anywhere. */
export const MODE: Mode = {
  name: 'browser',
  http: [],
  providers: [
    { provide: RecordingSource, useExisting: LocalFiles },
    { provide: StatsFiles, useExisting: LocalStatsFiles },
    { provide: ScoreHistory, useExisting: LocalScoreHistory },
    { provide: ReviewEngine, useExisting: BrowserReview },
    { provide: ModelCatalog, useExisting: BrowserModels },
    { provide: MouseLogs, useExisting: BrowserMouseLogs },
    { provide: Labelling, useExisting: BrowserLabelling },
    { provide: AreaLabels, useExisting: BrowserAreaLabels },
    { provide: FaintCutoffs, useExisting: BrowserFaintCutoffs },
  ],
};
