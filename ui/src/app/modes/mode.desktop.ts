import { withInterceptors, withXhr } from '@angular/common/http';
import { AreaLabels } from '../platform/area-labels';
import { FaintCutoffs } from '../platform/faint-cutoffs';
import { Mode } from '../platform/mode';
import { Labelling } from '../platform/labelling';
import { ModelCatalog } from '../platform/model-catalog';
import { MouseLogs } from '../platform/mouse-logs';
import { RecordingSource } from '../platform/recording-source';
import { ReviewEngine } from '../platform/review-engine';
import { StatsFiles } from '../platform/stats-files';
import { ServerAreaLabels } from './http/server-area-labels';
import { ServerFaintCutoffs } from './http/server-faint-cutoffs';
import { ServerLabelling } from './http/server-labelling';
import { ServerModels } from './http/server-models';
import { ServerReview } from './http/server-review';
import { ServerStatsFiles } from './http/server-stats-files';
import { desktopApi } from './tauri/desktop-api';
import { DesktopMouseLogs } from './tauri/desktop-mouse-logs';
import { DesktopRecordings } from './tauri/desktop-recordings';

/**
 * The desktop app (Tauri 2, desktop/): the review server's services, answered by the app itself (desktop/src/api.rs),
 * which reads the disk directly and runs the review natively. HttpClient sends with XMLHttpRequest, so an upload
 * reports its progress.
 */
export const MODE: Mode = {
  name: 'desktop',
  http: [withXhr(), withInterceptors([desktopApi])],
  providers: [
    { provide: RecordingSource, useExisting: DesktopRecordings },
    { provide: StatsFiles, useExisting: ServerStatsFiles },
    { provide: ReviewEngine, useExisting: ServerReview },
    { provide: ModelCatalog, useExisting: ServerModels },
    { provide: MouseLogs, useExisting: DesktopMouseLogs },
    { provide: Labelling, useExisting: ServerLabelling },
    { provide: AreaLabels, useExisting: ServerAreaLabels },
    { provide: FaintCutoffs, useExisting: ServerFaintCutoffs },
  ],
};
