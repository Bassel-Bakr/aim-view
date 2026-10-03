import { Motion, TrackSummary, WhatIf } from '../../api';
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

/** The run's cards. Bots that die add the switching between them. */
export function trackStats(s: TrackSummary): TrackStat[] {
  const bots = s.bots > 0;
  const stat = (label: string, value: string, why: string, detail = ''): TrackStat => ({
    label,
    value,
    detail,
    why,
  });
  const lostWhy =
    'How often you lost the bot: came off it for more than 0.1 s, per second of tracking.' +
    (s.lost_cost == null
      ? ''
      : ` The time off it in those drops cost you ${formatPercent(s.lost_cost)} accuracy` +
        `${s.slip_cost != null ? `; slips shorter than 0.1 s cost ${formatPercent(s.slip_cost)} more` : ''}.`);
  return [
    stat('Score', formatCount(s.score), "The run's score, from the stats file or the file name."),
    stat(
      bots ? 'On target while tracking' : 'On target',
      formatPercent(s.on_target),
      'How much of the time your crosshair was on the bot.' +
        (bots ? ' The time after a bot dies, until you are on the next one, is left out.' : ''),
    ),
    ...(bots
      ? [
          stat(
            'On target, whole run',
            formatPercent(s.on_all),
            "The same with that switching time counted too, as the game's accuracy counts it.",
          ),
        ]
      : []),
    stat(
      'Accuracy (stats file)',
      formatPercent(s.accuracy),
      "The game's own number: hits ÷ (hits + misses) while you fired. A check on On target.",
    ),
    stat(
      'Distance from the center',
      formatDegrees(s.error),
      "How far your crosshair usually was from the bot's middle (a capsule's middle line), when on or near it.",
    ),
    stat(
      'Lost the bot, per second',
      s.lost == null ? '–' : s.lost.toFixed(2),
      lostWhy,
      s.lost_cost == null ? '' : `cost ${formatPercent(s.lost_cost)} accuracy`,
    ),
    stat(
      'Time to get back',
      formatSeconds(s.back),
      'How long those drops usually lasted before you were back on.',
    ),
    stat('Longest off', formatSeconds(s.longest_off), 'The longest single drop off the bot.'),
    ...(bots
      ? [
          stat(
            'Bots killed',
            formatCount(s.bots),
            "Bots that died in the run (from the stats file or the HUD's kill count).",
          ),
          stat(
            'Time to the next bot',
            formatSeconds(s.to_next),
            "Usual time from a bot's death until you were on a bot again.",
          ),
          stat(
            'Waiting for a spawn',
            formatSeconds(s.waiting),
            'The part of that with no bot on screen yet.',
          ),
          stat(
            'Getting onto it',
            formatSeconds(s.onto),
            'The part from the next bot showing until you were on it.',
          ),
          stat(
            'Switching',
            formatPercent(s.switching),
            'How much of the run went on getting from a dead bot to the next.',
          ),
        ]
      : []),
    stat(
      'Game FPS',
      s.fps_avg ? String(Math.round(s.fps_avg)) : '–',
      'Your average frame rate, from the stats file.',
    ),
  ];
}

/** How the run's numbers were measured. */
export function trackNote(s: TrackSummary): string {
  const bots = s.bots > 0;
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
    (s.faint
      ? ` The faint-target cut-off is on: ${s.faint.tracks} tracks scoring under ${s.faint.cut} are left out of every measure here.`
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

const side = (v: number | null | undefined) =>
  v == null ? '–' : `${Math.abs(v).toFixed(2)}° ${v < 0 ? 'behind' : 'ahead'}`;
const rate = (v: number | null | undefined) => (v == null ? '–' : v.toFixed(2));

export function motionView(m: Motion | null): MotionView | null {
  if (!m) return null;
  const camera = `the camera's turn was read in ${formatPercent(m.camera)} of the frames`;
  const seconds = (m.seconds ?? 0).toFixed(1);
  if (m.reason)
    return {
      reason: `Not measured: ${m.reason} (${seconds} s); ${camera}.`,
      stats: [],
      rows: [],
      note: '',
    };
  const stat = (label: string, value: string, detail: string, why: string): TrackStat => ({
    label,
    value,
    detail,
    why,
  });
  return {
    reason: null,
    stats: [
      stat(
        'Behind or ahead',
        side(m.lag),
        m.lag_ms == null ? '' : `${Math.abs(Math.round(m.lag_ms))} ms at its speed`,
        "Where your crosshair usually sat along the bot's motion: behind means trailing it, ahead means leading it.",
      ),
      stat(
        'Off target: behind',
        formatPercent(m.off_behind),
        '',
        'Trailing: the share of the off-target time spent behind the bot.',
      ),
      stat(
        'Off target: ahead',
        formatPercent(m.off_ahead),
        '',
        'Leading: the share of the off-target time spent ahead of the bot, past its edge.',
      ),
      stat(
        'Off target: to the side',
        formatPercent(m.off_side),
        '',
        "Share of the off-target time spent beside the bot's path (above or below a bot moving sideways).",
      ),
      stat(
        'Overshoots a second',
        rate(m.overshoots),
        m.overshoot_dist == null ? '' : `${formatDegrees(m.overshoot_dist)} past the edge`,
        'How often you went ahead of the bot past its edge, per second of tracking.',
      ),
      stat(
        'Over-correcting',
        formatPercent(m.overcorrect),
        m.corrections == null ? '' : `${m.swing_count} of ${m.corrections} corrections`,
        "Of your corrections (each turn of your crosshair back toward the bot's middle), the share that went too far: " +
          "across the middle to the other side by half the bot's width or more, while the bot kept its direction.",
      ),
      stat(
        'Reaction to direction changes',
        m.reaction == null ? '–' : `${Math.round(m.reaction)} ms`,
        `${m.reversals ?? 0} changes`,
        'Usual time from the bot turning until your mouse moved the new way.',
      ),
      stat(
        'Carried past at direction changes',
        formatPercent(m.reversal_overshoot),
        m.reversal_overshoot_dist == null
          ? ''
          : `${formatDegrees(m.reversal_overshoot_dist)} past the edge`,
        'How often, when the bot turned, you kept going the old way past its edge.',
      ),
      stat(
        'Horizontal distance from the line',
        formatDegrees(m.error_h),
        '',
        "How far left or right of the bot's middle line you usually were, when on or near it.",
      ),
      stat(
        'Vertical distance from the line',
        formatDegrees(m.error_v),
        '',
        'How far above or below it you usually were. On a capsule, anywhere along its length counts as 0.',
      ),
      stat(
        'Target speed',
        m.target_speed == null ? '–' : `${Math.round(m.target_speed)} °/s`,
        'its own motion',
        'How fast the bot itself usually moved, apart from your mouse.',
      ),
    ],
    rows: (m.by_direction ?? []).map((b) => ({
      moving: `${DIRECTION_ARROWS[b.name]} ${b.name}`,
      time: formatPercent(b.share),
      on: formatPercent(b.on),
      distance: formatDegrees(b.distance),
      lag: side(b.lag),
    })),
    note:
      "Read from the video alone: the room's slide across the screen gives the camera's turn (your mouse), and the " +
      "target's move on screen less that gives its own motion. Measured while the target moves and your crosshair " +
      `is with it (within 2° of it, ${(m.seconds ?? 0).toFixed(0)} s here); ${camera}. Behind or ahead is the median ` +
      "offset along the target's motion. An overshoot is a stretch ahead of the target past its leading edge. A swing " +
      'is the crosshair crossing from behind the target to ahead of it, or back, by half its width or more, while it ' +
      'keeps its direction. At a direction change, the reaction is the time until your mouse moves the new way, and ' +
      '"carried past" counts the changes after which the crosshair went on the old way past the target\'s edge. ' +
      "Distances are from the target's center line (a sphere's center, a capsule's long axis).",
  };
}

/** A row of the what-if table, as shown. */
export interface WhatIfRow {
  what: string;
  gain: string;
  how: string;
}

export function whatIfRows(w: WhatIf[]): WhatIfRow[] {
  return w.map((r) => ({ what: r.what, gain: `+${(100 * r.gain).toFixed(1)}%`, how: r.how }));
}
