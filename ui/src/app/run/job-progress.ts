import { Job, JobStage } from '../api';

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
  ffmpeg: 'Getting FFmpeg (once)',
  'reading the HUD': 'Reading the session HUD',
  camera: "Reading the camera's turn",
  measuring: 'Measuring',
  // a link's download: the video's own progress shows it (RecordingSource), not the review's
  downloading: 'Downloading the video',
  'yt-dlp': 'Getting yt-dlp (once)',
  done: 'Reviewed',
  error: 'The review failed',
};

/**
 * The progress line of a review job, or null when there is none. While it tracks and when it is done, the stage names
 * the device the detector ran on, when the job says it ("Tracking the targets on DirectML").
 */
export function jobProgress(job: Job): JobProgress | null {
  if (job.stage === 'none') return null;
  const failed = job.stage === 'error';
  const frames = job.stage === 'tracking' || job.stage === 'camera';
  const on =
    job.device && (job.stage === 'tracking' || job.stage === 'done') ? ` on ${job.device}` : '';
  return {
    stage: failed ? `${STAGES.error}: ${job.error}` : STAGES[job.stage] + on,
    count:
      job.stage === 'done'
        ? `in ${job.seconds} s`
        : frames && job.total
          ? `${job.done} / ${job.total} frames`
          : '',
    fraction: job.stage === 'done' ? 1 : job.total ? (job.done ?? 0) / job.total : 0,
    failed,
  };
}
