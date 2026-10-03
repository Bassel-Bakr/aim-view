import { withInterceptors, withXhr } from '@angular/common/http';
import { Mode } from '../platform/mode';
import { ModelCatalog } from '../platform/model-catalog';
import { RecordingSource } from '../platform/recording-source';
import { ReviewEngine } from '../platform/review-engine';
import { StatsFiles } from '../platform/stats-files';
import { ServerModels } from './http/server-models';
import { ServerReview } from './http/server-review';
import { ServerStatsFiles } from './http/server-stats-files';
import { desktopApi } from './tauri/desktop-api';
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
  ],
};
