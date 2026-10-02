import { Component, computed, inject, input, signal } from '@angular/core';
import { Button } from '../controls/button';
import { JobStage, Recording } from '../api';
import { formatPercent } from '../format';
import { Library } from '../services/library';
import { modelName, Models } from '../services/models';
import { FlickList } from './flick-list/flick-list';
import { Player } from './player/player';
import { ClickReport } from './report/click-report';
import { TrackReport } from './report/track-report';
import { Review } from '../services/review';
import { RunHeader } from './run-header/run-header';
import { SpeedChart } from './speed-chart/speed-chart';
import { StatsFile } from './stats-file/stats-file';
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
  imports: [
    RunHeader,
    StatsFile,
    Player,
    Timeline,
    FlickList,
    SpeedChart,
    ClickReport,
    TrackReport,
    Button,
  ],
  templateUrl: './run.html',
  styleUrl: './run.scss',
})
export class Run {
  readonly recording = input.required<Recording>();
  protected readonly review = inject(Review);
  private readonly models = inject(Models);
  private readonly library = inject(Library);

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
  protected readonly clickReport = computed(() => {
    const r = this.report();
    return r?.mode === 'click' ? r : null;
  });
  protected readonly statsOpen = signal(false);
  /** The recording's video, which may still be being remuxed into MP4. */
  protected readonly video = computed(() => this.library.source.video(this.recording().id));
  /** Where the player reads the video; null while it is being remuxed. */
  protected readonly videoUrl = computed<string | null>(() => {
    const v = this.video();
    return !v || v.state === 'remuxing' ? null : v.url;
  });
  protected readonly remuxShare = computed(() => {
    const v = this.video();
    return v?.state === 'remuxing' ? formatPercent(v.progress) : '';
  });
  /** Reviewed, but its report is gone (a new stats file): measured again on its tracks, not reviewed again. */
  private readonly unmeasured = computed(
    () =>
      this.recording().analysed &&
      this.review.report.hasValue() &&
      this.review.report.value() === null,
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
    if (this.unmeasured()) return 'Measure again';
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
    void this.review.analyse(this.recording().analysed && !this.unmeasured());
  }

  protected toggleStats(): void {
    this.statsOpen.update((open) => !open);
  }
}
