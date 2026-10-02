import { withXhr } from '@angular/common/http';
import { Mode } from '../platform/mode';
import { ModelCatalog } from '../platform/model-catalog';
import { RecordingSource } from '../platform/recording-source';
import { ReviewEngine } from '../platform/review-engine';
import { StatsFiles } from '../platform/stats-files';
import { ServerModels } from './http/server-models';
import { ServerRecordings } from './http/server-recordings';
import { ServerReview } from './http/server-review';
import { ServerStatsFiles } from './http/server-stats-files';

/**
 * With the review server (python/server.py): its recordings, stats files, models and reviews, and what the user
 * sets, kept there. HttpClient sends with XMLHttpRequest here, since fetch cannot report an upload's progress.
 */
export const MODE: Mode = {
  name: 'server',
  http: [withXhr()],
  providers: [
    { provide: RecordingSource, useExisting: ServerRecordings },
    { provide: StatsFiles, useExisting: ServerStatsFiles },
    { provide: ReviewEngine, useExisting: ServerReview },
    { provide: ModelCatalog, useExisting: ServerModels },
  ],
};
