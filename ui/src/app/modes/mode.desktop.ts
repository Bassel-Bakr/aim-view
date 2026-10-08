/**
 * Desktop mode: the Tauri 2 app (desktop/). In: nothing at load. Out: `MODE`, which picks a service
 * for each platform/ contract, mostly the server mode's, with the app's own folder dialog and mouse
 * logger (tauri/). The `desktop` build configuration swaps mode.ts for this file.
 */

import { withInterceptors, withXhr } from '@angular/common/http';
import { AreaLabels } from '../platform/area-labels';
import { CropSets } from '../platform/crop-sets';
import { FaintCutoffs } from '../platform/faint-cutoffs';
import { Mode } from '../platform/mode';
import { Labelling } from '../platform/labelling';
import { ModelCatalog } from '../platform/model-catalog';
import { MouseLogs } from '../platform/mouse-logs';
import { RecordingSource } from '../platform/recording-source';
import { ReviewEngine } from '../platform/review-engine';
import { ScoreHistory } from '../platform/score-history';
import { StatsFiles } from '../platform/stats-files';
import { StoredData } from '../platform/stored-data';
import { ServerAreaLabels } from './http/server-area-labels';
import { ServerCropSets } from './http/server-crop-sets';
import { ServerFaintCutoffs } from './http/server-faint-cutoffs';
import { ServerLabelling } from './http/server-labelling';
import { ServerModels } from './http/server-models';
import { ServerReview } from './http/server-review';
import { ServerScoreHistory } from './http/server-score-history';
import { ServerStoredData } from './http/server-stored-data';
import { ServerStatsFiles } from './http/server-stats-files';
import { desktopApi } from './tauri/desktop-api';
import { DesktopMouseLogs } from './tauri/desktop-mouse-logs';
import { DesktopRecordings } from './tauri/desktop-recordings';

/**
 * The desktop app (Tauri 2, desktop/): the review server's services, answered by the app itself
 * (service/, through desktop/src/protocol.rs), which reads the disk directly and runs the review
 * natively. HttpClient sends with XMLHttpRequest, so an upload reports its progress, and the
 * `desktopApi` interceptor sends the /api requests to the app.
 */
export const MODE: Mode = {
  name: 'desktop',
  http: [withXhr(), withInterceptors([desktopApi])],
  providers: [
    { provide: RecordingSource, useExisting: DesktopRecordings },
    { provide: StatsFiles, useExisting: ServerStatsFiles },
    { provide: ScoreHistory, useExisting: ServerScoreHistory },
    { provide: StoredData, useExisting: ServerStoredData },
    { provide: ReviewEngine, useExisting: ServerReview },
    { provide: ModelCatalog, useExisting: ServerModels },
    { provide: MouseLogs, useExisting: DesktopMouseLogs },
    { provide: Labelling, useExisting: ServerLabelling },
    { provide: AreaLabels, useExisting: ServerAreaLabels },
    { provide: FaintCutoffs, useExisting: ServerFaintCutoffs },
    { provide: CropSets, useExisting: ServerCropSets },
  ],
};
