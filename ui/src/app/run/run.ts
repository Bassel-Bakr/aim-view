import { Component, computed, inject, input } from '@angular/core';
import { JobStage, Recording } from '../api';
import { modelName, Models } from '../services/models';
import { Player } from './player/player';
import { Review } from './review';
import { RunHeader } from './run-header/run-header';
import { Timeline } from './timeline/timeline';

/** A review job's progress as the page shows it: the stage (announced) and the frames done (not announced). */
export interface JobProgress {
  stage: string;
  count: string;
  fraction: number;
  failed: boolean;
}

const STAGES: Record<JobStage, string> = {
  none: '',
  starting: 'Starting',
  looking: 'Looking at the key frames',
  tracking: 'Tracking the targets',
  linking: 'Linking the tracks',
  'reading the HUD': 'Reading the session HUD',
  camera: "Reading the camera's turn",
  measuring: 'Measuring',
  done: 'Reviewed',
  error: 'The review failed',
};

/** The open recording: its header, the review's button and progress, the video, and a tracking run's timeline. */
@Component({
  selector: 'app-run',
  imports: [RunHeader, Player, Timeline],
  templateUrl: './run.html',
})
export class Run {
  readonly recording = input.required<Recording>();
  protected readonly review = inject(Review);
  private readonly models = inject(Models);

  protected readonly report = computed(() =>
    this.review.report.hasValue() ? this.review.report.value() : null,
  );
  protected readonly tracks = computed(() =>
    this.review.tracks.hasValue() ? this.review.tracks.value() : null,
  );
  protected readonly trackReport = computed(() => {
    const r = this.report();
    return r?.mode === 'track' ? r : null;
  });
  protected readonly videoUrl = computed(
    () => `/video?id=${encodeURIComponent(this.recording().id)}`,
  );

  /** Which model made the review on screen, and whether it is the one new reviews use. */
  protected readonly reviewedBy = computed<string | null>(() => {
    const r = this.report();
    if (!r) return null;
    const by = r.review_model;
    if (by === null)
      return 'The model behind this review was not recorded (it is from before reviews kept it)';
    const chosen = this.models.chosen();
    const other = chosen && chosen !== by ? `, not ${modelName(chosen)} (the model in use)` : '';
    return `Reviewed with ${modelName(by)}${other}`;
  });

  protected readonly actionLabel = computed(() => {
    if (!this.recording().analysed) return 'Analyse';
    const chosen = this.models.chosen();
    const by = this.report()?.review_model;
    return chosen && by !== chosen ? `Review with ${modelName(chosen)}` : 'Review again';
  });

  protected readonly progress = computed<JobProgress | null>(() => {
    const job = this.review.job();
    if (job.stage === 'none') return null;
    const failed = job.stage === 'error';
    const frames = job.stage === 'tracking' || job.stage === 'camera';
    return {
      stage: failed ? `${STAGES.error}: ${job.error}` : STAGES[job.stage],
      count:
        job.stage === 'done'
          ? `in ${job.seconds} s`
          : frames && job.total
            ? `${job.done} / ${job.total} frames`
            : '',
      fraction: job.stage === 'done' ? 1 : job.total ? (job.done ?? 0) / job.total : 0,
      failed,
    };
  });

  protected startReview(): void {
    void this.review.analyse(this.recording().analysed);
  }
}
