import { Motion, TrackSummary, WhatIf } from '../../api';
import { WhatIfTable } from '../what-if-section/what-if-section';
import {
  DIRECTION_ARROWS,
  formatCount,
  formatDegrees,
  formatPercent,
  formatSeconds,
} from '../../format';

/** A tracking card: its value, what it is, a detail under it, and why it matters (shown, not only on hover). */
export interface TrackStat {
  label: string;
  value: string;
  detail: string;
  why: string;
}

/** How many of the run's cards go above the video, as its headline; the report shows the rest. */
export const HEADLINE_TILES = 6;

/** A card from its label, its value, why it matters and the detail under it. */
function card(label: string, value: string, why: string, detail = ''): TrackStat {
  return { label, value, detail, why };
}

/** Why losing the bot matters, with what the drops and the slips cost where the review measured it. */
function lostWhy(summary: TrackSummary): string {
  if (summary.lost_cost == null) {
    return 'How often you lost the bot: came off it for more than 0.1 s, per second of tracking.';
  }
  const slips =
    summary.slip_cost != null
      ? `; slips shorter than 0.1 s cost ${formatPercent(summary.slip_cost)} more`
      : '';
  return (
    'How often you lost the bot: came off it for more than 0.1 s, per second of tracking.' +
    ` The time off it in those drops cost you ${formatPercent(summary.lost_cost)} accuracy${slips}.`
  );
}

/** The cards on the time on the bot: the score, on target, accuracy and the distance from the center. */
function onTargetCards(summary: TrackSummary, bots: boolean): TrackStat[] {
  return [
    card(
      'Score',
      formatCount(summary.score),
      "The run's score, from the stats file or the file name.",
    ),
    card(
      bots ? 'On target while tracking' : 'On target',
      formatPercent(summary.on_target),
      'How much of the time your crosshair was on the bot.' +
        (bots ? ' The time after a bot dies, until you are on the next one, is left out.' : ''),
    ),
    ...(bots
      ? [
          card(
            'On target, whole run',
            formatPercent(summary.on_all),
            "The same with that switching time counted too, as the game's accuracy counts it.",
          ),
        ]
      : []),
    card(
      'Accuracy (stats file)',
      formatPercent(summary.accuracy),
      "The game's own number: hits ÷ (hits + misses) while you fired. A check on On target.",
    ),
    card(
      'Distance from the center',
      formatDegrees(summary.error),
      "How far your crosshair usually was from the bot's middle (a capsule's middle line), when on or near it.",
    ),
  ];
}

/** The cards on the drops off the bot. */
function lostCards(summary: TrackSummary): TrackStat[] {
  return [
    card(
      'Lost the bot, per second',
      summary.lost == null ? '–' : summary.lost.toFixed(2),
      lostWhy(summary),
      summary.lost_cost == null ? '' : `cost ${formatPercent(summary.lost_cost)} accuracy`,
    ),
    card(
      'Time to get back',
      formatSeconds(summary.back),
      'How long those drops usually lasted before you were back on.',
    ),
    card('Longest off', formatSeconds(summary.longest_off), 'The longest single drop off the bot.'),
  ];
}

/** The cards on the switching between bots, where bots die. */
function switchingCards(summary: TrackSummary): TrackStat[] {
  return [
    card(
      'Bots killed',
      formatCount(summary.bots),
      "Bots that died in the run (from the stats file or the HUD's kill count).",
    ),
    card(
      'Time to the next bot',
      formatSeconds(summary.to_next),
      "Usual time from a bot's death until you were on a bot again.",
    ),
    card(
      'Waiting for a spawn',
      formatSeconds(summary.waiting),
      'The part of that with no bot on screen yet.',
    ),
    card(
      'Getting onto it',
      formatSeconds(summary.onto),
      'The part from the next bot showing until you were on it.',
    ),
    card(
      'Switching',
      formatPercent(summary.switching),
      'How much of the run went on getting from a dead bot to the next.',
    ),
  ];
}

/** The run's cards. Bots that die add the switching between them. */
export function trackStats(summary: TrackSummary): TrackStat[] {
  const bots = summary.bots > 0;
  return [
    ...onTargetCards(summary, bots),
    ...lostCards(summary),
    ...(bots ? switchingCards(summary) : []),
    card(
      'Game FPS',
      summary.fps_avg ? String(Math.round(summary.fps_avg)) : '–',
      'Your average frame rate, from the stats file.',
    ),
  ];
}

/** How the run's numbers were measured. */
export function trackNote(summary: TrackSummary): string {
  const bots = summary.bots > 0;
  return (
    'Tracking: the review measures the time the crosshair spent on the target, from the tracked targets. On target ' +
    "means the crosshair lay inside the target's box. Distance from the center is the median distance from the " +
    "target's center line (a sphere's center, a capsule's long axis) over the frames on or near it. Lost the bot " +
    'counts the stretches off the target longer than 0.1 s, per second of tracking, with the accuracy they cost ' +
    "(their time as a share of the tracking time); time to get back is their median length. The stats file's " +
    "accuracy is the game's own measure of the same thing." +
    (bots
      ? ' Bots die here: from a death until your crosshair is on a target again is switching, not tracking, so it is ' +
        'left out of "while tracking", the lost stretches and the measures below, and measured on its own. It splits ' +
        'into waiting for a spawn (no target on screen) and getting onto it. "Whole run" counts switching too, as the ' +
        'accuracy does.'
      : '') +
    (summary.faint
      ? summary.faint.cut === null
        ? ' The faint-target cut-off is on, but this review has no detector scores: nothing is left out.'
        : ` The faint-target cut-off is on: ${summary.faint.tracks} tracks scoring under ${summary.faint.cut} are left out of every measure here.`
      : '')
  );
}

/** A row of the by-direction table, as shown. */
export interface MotionRow {
  moving: string;
  time: string;
  on: string;
  distance: string;
  lag: string;
}

/** How the crosshair followed the bot: its cards and rows, or why it was not measured. */
export interface MotionView {
  reason: string | null;
  stats: TrackStat[];
  rows: MotionRow[];
  note: string;
}

const side = (offsetDeg: number | null | undefined) =>
  offsetDeg == null
    ? '–'
    : `${Math.abs(offsetDeg).toFixed(2)}° ${offsetDeg < 0 ? 'behind' : 'ahead'}`;
const rate = (perSecond: number | null | undefined) =>
  perSecond == null ? '–' : perSecond.toFixed(2);

/** A motion card from its label, its value, the detail under it and why it matters. */
function motionCard(label: string, value: string, detail: string, why: string): TrackStat {
  return card(label, value, why, detail);
}

/** The cards on where the crosshair sat along the bot's motion: behind, ahead, to the side, past its edge. */
function followCards(motion: Motion): TrackStat[] {
  return [
    motionCard(
      'Behind or ahead',
      side(motion.lag),
      motion.lag_ms == null ? '' : `${Math.abs(Math.round(motion.lag_ms))} ms at its speed`,
      "Where your crosshair usually sat along the bot's motion: behind means trailing it, ahead means leading it.",
    ),
    motionCard(
      'Off target: behind',
      formatPercent(motion.off_behind),
      '',
      'Trailing: the share of the off-target time spent behind the bot.',
    ),
    motionCard(
      'Off target: ahead',
      formatPercent(motion.off_ahead),
      '',
      'Leading: the share of the off-target time spent ahead of the bot, past its edge.',
    ),
    motionCard(
      'Off target: to the side',
      formatPercent(motion.off_side),
      '',
      "Share of the off-target time spent beside the bot's path (above or below a bot moving sideways).",
    ),
    motionCard(
      'Overshoots a second',
      rate(motion.overshoots),
      motion.overshoot_dist == null ? '' : `${formatDegrees(motion.overshoot_dist)} past the edge`,
      'How often you went ahead of the bot past its edge, per second of tracking.',
    ),
  ];
}

/** The cards on correcting: over-correcting, turns, the distances from the line and the bot's own speed. */
function correctionCards(motion: Motion): TrackStat[] {
  return [
    motionCard(
      'Over-correcting',
      formatPercent(motion.overcorrect),
      motion.corrections == null
        ? ''
        : `${motion.swing_count} of ${motion.corrections} corrections`,
      "Of your corrections (each turn of your crosshair back toward the bot's middle), the share that went too far: " +
        "across the middle to the other side by half the bot's width or more, while the bot kept its direction.",
    ),
    motionCard(
      'Reaction to direction changes',
      motion.reaction == null ? '–' : `${Math.round(motion.reaction)} ms`,
      `${motion.reversals ?? 0} changes`,
      'Usual time from the bot turning until your mouse moved the new way.',
    ),
    motionCard(
      'Carried past at direction changes',
      formatPercent(motion.reversal_overshoot),
      motion.reversal_overshoot_dist == null
        ? ''
        : `${formatDegrees(motion.reversal_overshoot_dist)} past the edge`,
      'How often, when the bot turned, you kept going the old way past its edge.',
    ),
    motionCard(
      'Horizontal distance from the line',
      formatDegrees(motion.error_h),
      '',
      "How far left or right of the bot's middle line you usually were, when on or near it.",
    ),
    motionCard(
      'Vertical distance from the line',
      formatDegrees(motion.error_v),
      '',
      'How far above or below it you usually were. On a capsule, anywhere along its length counts as 0.',
    ),
    motionCard(
      'Target speed',
      motion.target_speed == null ? '–' : `${Math.round(motion.target_speed)} °/s`,
      'its own motion',
      'How fast the bot itself usually moved, apart from your mouse.',
    ),
  ];
}

/** How the following was measured, and what its words mean. */
function motionNote(motion: Motion, camera: string): string {
  return (
    "Read from the video alone: the room's slide across the screen gives the camera's turn (your mouse), and the " +
    "target's move on screen less that gives its own motion. Measured while the target moves and your crosshair " +
    `is with it (within 2° of it, ${(motion.seconds ?? 0).toFixed(0)} s here); ${camera}. Behind or ahead is the median ` +
    "offset along the target's motion. An overshoot is a stretch ahead of the target past its leading edge. A swing " +
    'is the crosshair crossing from behind the target to ahead of it, or back, by half its width or more, while it ' +
    'keeps its direction. At a direction change, the reaction is the time until your mouse moves the new way, and ' +
    '"carried past" counts the changes after which the crosshair went on the old way past the target\'s edge. ' +
    "Distances are from the target's center line (a sphere's center, a capsule's long axis)."
  );
}

export function motionView(motion: Motion | null): MotionView | null {
  if (!motion) return null;
  const camera = `the camera's turn was read in ${formatPercent(motion.camera)} of the frames`;
  const seconds = (motion.seconds ?? 0).toFixed(1);
  if (motion.reason)
    return {
      reason: `Not measured: ${motion.reason} (${seconds} s); ${camera}.`,
      stats: [],
      rows: [],
      note: '',
    };
  return {
    reason: null,
    stats: [...followCards(motion), ...correctionCards(motion)],
    rows: (motion.by_direction ?? []).map((band) => ({
      moving: `${DIRECTION_ARROWS[band.name]} ${band.name}`,
      time: formatPercent(band.share),
      on: formatPercent(band.on),
      distance: formatDegrees(band.distance),
      lag: side(band.lag),
    })),
    note: motionNote(motion, camera),
  };
}

/** The what-if table: the accuracy each change would add. */
export function whatIfTable(whatIfs: WhatIf[]): WhatIfTable {
  const lines = whatIfs.map((whatIf) => ({
    what: whatIf.what,
    gains: [`+${(100 * whatIf.gain).toFixed(1)}%`],
    how: whatIf.how,
  }));
  return { columns: ['Accuracy'], groups: lines.length ? [{ name: null, lines }] : [] };
}
