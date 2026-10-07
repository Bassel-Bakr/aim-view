import { Component, computed, inject, input, signal } from '@angular/core';
import { Button } from '../controls/button';
import { isClickReport, Recording } from '../api';
import { formatPercent } from '../format';
import { QueueBar } from '../labelling/queue-bar/queue-bar';
import { FaintCutoff } from '../services/faint-cutoff';
import { Library } from '../services/library';
import { modelName, Models } from '../services/models';
import { AreaBar } from './areas/area-bar/area-bar';
import { AreaCanvas } from './areas/area-canvas/area-canvas';
import { AreaDraft } from './areas/area-draft';
import { ClickSide } from './click-side/click-side';
import { FlickList } from './flick-list/flick-list';
import { Headline } from './headline/headline';
import { jobProgress } from './job-progress';
import { scoreChange } from './score-change';
import { KillLanes } from './kill-lanes/kill-lanes';
import { MousePanel } from './mouse-panel/mouse-panel';
import { Player } from './player/player';
import { ProgressChart } from './progress-chart/progress-chart';
import { ClickReport } from './report/click-report';
import { HeadlineTile, runHeadline } from './report/click-stats';
import { HEADLINE_TILES, trackStats } from './report/track-stats';
import { TrackReport } from './report/track-report';
import { Review } from '../services/review';
import { RunHeader } from './run-header/run-header';
import { FaintCutoffPanel } from './faint-cutoff/faint-cutoff-panel';
import { formatClock, RunWindow } from './run-window/run-window';
import { SpeedChart } from './speed-chart/speed-chart';
import { StatsFile } from './stats-file/stats-file';
import { Timeline } from './timeline/timeline';

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
    ClickSide,
    Headline,
    ProgressChart,
    KillLanes,
    RunWindow,
    FaintCutoffPanel,
    TrackReport,
    MousePanel,
    Button,
    QueueBar,
    AreaBar,
    AreaCanvas,
  ],
  templateUrl: './run.html',
  styleUrl: './run.scss',
})
export class Run {
  readonly recording = input.required<Recording>();
  protected readonly review = inject(Review);
  /** The excluded areas editor: its bar above the video, the areas over it. */
  protected readonly areas = inject(AreaDraft);
  private readonly models = inject(Models);
  private readonly library = inject(Library);

  protected readonly report = computed(() =>
    this.review.report.hasValue() ? this.review.report.value() : null,
  );
  /** The faint-target cut-off: its panel, and the tracks without those it leaves out. */
  protected readonly faint = inject(FaintCutoff);
  /** The tracks the page shows: without those the faint-target cut-off leaves out. */
  protected readonly tracks = this.faint.tracks;
  protected readonly trackReport = computed(() => {
    const report = this.report();
    return report?.mode === 'track' ? report : null;
  });
  protected readonly clickReport = computed(() => {
    const report = this.report();
    return isClickReport(report) ? report : null;
  });
  /** The run in a few numbers, above the video: a tracking run's are its first cards (track-report shows the rest). */
  protected readonly headline = computed<HeadlineTile[] | null>(() => {
    const clicking = this.clickReport();
    const tracking = this.trackReport();
    const tiles = clicking
      ? runHeadline(clicking.summary, clicking.issues, clicking.flicks, clicking.fps)
      : tracking
        ? trackStats(tracking.summary)
            .slice(0, HEADLINE_TILES)
            .map((stat) => ({
              label: stat.label,
              value: stat.value,
              note: stat.detail,
              attention: false,
              good: false,
              why: stat.why,
            }))
        : null;
    // the score's line: against the same scenario's run before, where there is one
    const change = scoreChange(this.recording(), this.library.source.recordings());
    return tiles && change
      ? tiles.map((tile, i) => (i === 0 ? { ...tile, note: change.text, good: change.up } : tile))
      : tiles;
  });
  protected readonly statsOpen = signal(false);
  protected readonly windowOpen = signal(false);
  /** The run window's button: the marked times, or what it does when none are. */
  protected readonly windowLabel = computed(() => {
    const marks = this.review.marks.hasValue() ? this.review.marks.value() : null;
    if (!marks || (marks.start == null && marks.end == null)) return 'Run window';
    const at = (seconds: number | null) => (seconds == null ? '…' : formatClock(seconds));
    return `Run ${at(marks.start)}–${at(marks.end)}`;
  });
  /** The recording's video, which may still be being remuxed into MP4. */
  protected readonly video = computed(() => this.library.source.video(this.recording().id));
  /** Where the player reads the video; null while it is being remuxed or downloaded. */
  protected readonly videoUrl = computed<string | null>(() => {
    const video = this.video();
    return video?.state === 'ready' || video?.state === 'failed' ? video.url : null;
  });
  protected readonly remuxShare = computed(() => {
    const video = this.video();
    return video?.state === 'remuxing' ? formatPercent(video.progress) : '';
  });
  /** A video from a link that is not here yet (downloading, or the download failed): nothing to review. */
  protected readonly notHere = computed(() => {
    const state = this.video()?.state;
    return state === 'downloading' || state === 'not-downloaded';
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
    const report = this.report();
    if (!report) return null;
    const by = report.review_model;
    if (by === null)
      return 'The model behind this review was not recorded (it is from before reviews kept it)';
    const chosen = this.models.chosen();
    const other = chosen && chosen !== by ? `, not ${modelName(chosen)} (the model in use)` : '';
    // the device in the review's detector, as tracks.json names it: "onnxruntime (DirectML)", "onnxruntime-web (WebGPU)"
    const device = /\(([^)]+)\)$/.exec(this.tracks()?.detector ?? '')?.[1];
    return `Reviewed with ${modelName(by)}${device ? ` on ${device}` : ''}${other}`;
  });

  protected readonly actionLabel = computed(() => {
    if (!this.recording().analysed) return 'Analyze';
    if (this.unmeasured()) return 'Measure again';
    const chosen = this.models.chosen();
    const by = this.report()?.review_model;
    return chosen && by !== chosen ? `Review with ${modelName(chosen)}` : 'Review again';
  });

  protected readonly progress = computed(() => jobProgress(this.review.job()));

  protected startReview(): void {
    void this.review.analyse(this.recording().analysed && !this.unmeasured());
  }

  protected cancelReview(): void {
    void this.review.cancel();
  }

  protected cancelDownload(): void {
    void this.library.source.cancelLink(this.recording().id);
  }

  protected toggleWindow(): void {
    this.windowOpen.update((open) => !open);
  }

  /** Opens the cut-off's panel, or closes it (it stays while the cut-off is on). */
  protected toggleCutoff(): void {
    this.faint.asked.update((open) => !open);
  }

  protected toggleStats(): void {
    this.statsOpen.update((open) => !open);
  }

  protected toggleAreas(): void {
    if (this.areas.open()) this.areas.stop();
    else this.areas.start(this.recording().id);
  }
}
