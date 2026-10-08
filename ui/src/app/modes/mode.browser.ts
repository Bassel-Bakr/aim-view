/**
 * Browser mode: everything runs in the page. In: nothing at load. Out: `MODE`, which picks a
 * service for each platform/ contract (the browser classes in service/, else the server mode's)
 * and the `serviceApi` interceptor. The `browser` build configuration swaps mode.ts for this file,
 * and mode.ts loads it when no configuration swaps it (the tests).
 */

import { withInterceptors } from '@angular/common/http';
import { AreaLabels } from '../platform/area-labels';
import { CropSets } from '../platform/crop-sets';
import { FaintCutoffs } from '../platform/faint-cutoffs';
import { Exports } from '../platform/exports';
import { Mode, MODE_NAME } from '../platform/mode';
import { Labelling } from '../platform/labelling';
import { ModelCatalog } from '../platform/model-catalog';
import { MouseLogs } from '../platform/mouse-logs';
import { RecordingSource } from '../platform/recording-source';
import { ReviewEngine } from '../platform/review-engine';
import { ScoreHistory } from '../platform/score-history';
import { StatsFiles } from '../platform/stats-files';
import { StoredData } from '../platform/stored-data';
import { ServerModels } from './http/server-models';
import { ServerScoreHistory } from './http/server-score-history';
import { ServerExports } from './http/server-exports';
import { ServerStoredData } from './http/server-stored-data';
import { BrowserAreaLabels } from './service/browser-area-labels';
import { BrowserCropSets } from './service/browser-crop-sets';
import { BrowserFaintCutoffs } from './service/browser-faint-cutoffs';
import { BrowserLabelling } from './service/browser-labelling';
import { BrowserMouseLogs } from './service/browser-mouse-logs';
import { BrowserRecordings } from './service/browser-recordings';
import { BrowserReview } from './service/browser-review';
import { BrowserStatsFiles } from './service/browser-stats-files';
import { serviceApi } from './service/service-api';

/**
 * Everything in the browser: the review server's services (modes/http/), answered by the review
 * service itself (the same Rust as the server and the desktop app) built as WebAssembly and run in
 * a worker of the page's own (service/service.worker.ts). Its files are kept in this browser;
 * nothing is sent anywhere. Where the page must act, a browser class extends the server's: opening
 * the VODs folder and playing its videos, copying KovaaK's folders in, running the review, the area
 * finder and the cut-off's labels in workers, and downloading links.
 */
export const MODE: Mode = {
  name: 'browser',
  http: [withInterceptors([serviceApi])],
  providers: [
    { provide: RecordingSource, useExisting: BrowserRecordings },
    { provide: StatsFiles, useExisting: BrowserStatsFiles },
    { provide: ScoreHistory, useExisting: ServerScoreHistory },
    { provide: StoredData, useExisting: ServerStoredData },
    { provide: MODE_NAME, useValue: 'browser' },
    { provide: Exports, useExisting: ServerExports },
    { provide: ReviewEngine, useExisting: BrowserReview },
    { provide: ModelCatalog, useExisting: ServerModels },
    { provide: MouseLogs, useExisting: BrowserMouseLogs },
    { provide: Labelling, useExisting: BrowserLabelling },
    { provide: AreaLabels, useExisting: BrowserAreaLabels },
    { provide: FaintCutoffs, useExisting: BrowserFaintCutoffs },
    { provide: CropSets, useExisting: BrowserCropSets },
  ],
};
