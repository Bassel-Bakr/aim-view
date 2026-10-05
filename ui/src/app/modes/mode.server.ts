import { withXhr } from '@angular/common/http';
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
import { ServerAreaLabels } from './http/server-area-labels';
import { ServerCropSets } from './http/server-crop-sets';
import { ServerFaintCutoffs } from './http/server-faint-cutoffs';
import { ServerLabelling } from './http/server-labelling';
import { ServerModels } from './http/server-models';
import { ServerMouseLogs } from './http/server-mouse-logs';
import { ServerRecordings } from './http/server-recordings';
import { ServerReview } from './http/server-review';
import { ServerScoreHistory } from './http/server-score-history';
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
    { provide: ScoreHistory, useExisting: ServerScoreHistory },
    { provide: ReviewEngine, useExisting: ServerReview },
    { provide: ModelCatalog, useExisting: ServerModels },
    { provide: MouseLogs, useExisting: ServerMouseLogs },
    { provide: Labelling, useExisting: ServerLabelling },
    { provide: AreaLabels, useExisting: ServerAreaLabels },
    { provide: FaintCutoffs, useExisting: ServerFaintCutoffs },
    { provide: CropSets, useExisting: ServerCropSets },
  ],
};
