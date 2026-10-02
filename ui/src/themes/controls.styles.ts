import { tv } from 'tailwind-variants/lite';

// The app's controls, shared by every page. Without tailwind-merge, a style sets a property in its base or in its
// variants, never both.

/** A button. intent: normal, or primary for the one main action on a page. */
export const button = tv({
  base: 'inline-flex min-h-(--control-height) items-center gap-3 rounded-md border px-6 font-strong whitespace-nowrap transition-colors',
  variants: {
    intent: {
      normal: 'border-border-strong bg-surface-2 text-primary hover:enabled:bg-surface-3',
      primary: 'border-accent bg-accent text-on-accent hover:enabled:bg-accent-hover',
    },
  },
  defaultVariants: { intent: 'normal' },
});

/** A filter chip: a small round button that says whether it is pressed (aria-pressed). */
export const chip = tv({
  base: 'min-h-(--control-height-sm) rounded-pill border border-border-strong px-5 py-1 text-xs text-secondary aria-pressed:border-accent aria-pressed:bg-surface-3 aria-pressed:text-primary',
});

/** A status in words, never by color alone. tone: neutral, or good (reviewed, done). */
export const badge = tv({
  base: 'rounded-pill px-3 leading-(--badge-height)',
  variants: {
    tone: {
      neutral: 'bg-surface-3 text-secondary',
      good: 'bg-good-soft text-good-text',
    },
  },
  defaultVariants: { tone: 'neutral' },
});

/** A number card: its value, what it is, and a line under it (the run's median, a detail, or why it matters). */
export const card = tv({
  slots: {
    root: 'flex flex-col gap-1 rounded-lg border border-border bg-surface-1 px-6 py-5',
    value: 'text-(length:--stat-size) font-strong text-value tabular-nums',
    label: 'font-strong text-primary',
    detail: 'text-xs text-muted',
  },
});

/** A small color square in a legend; its color comes from a background class or style. */
export const swatch = tv({
  base: 'inline-block size-(--swatch-size) flex-none rounded-(--swatch-radius)',
});

/** A note under a section: what it measures and how. */
export const note = tv({ base: 'mt-3 max-w-(--prose-width) text-xs text-muted' });

/** A label in an outlined pill: a run's kind, its data source, the model in use. */
export const pill = tv({
  base: 'rounded-pill border border-border-strong px-4 py-1 text-xs font-regular text-secondary',
});

/** One choice of a few: a group of buttons, each saying whether it is pressed (aria-pressed). */
export const segmented = tv({
  slots: {
    group: 'inline-flex rounded-md border border-border-strong bg-surface-2 p-1',
    option:
      'min-h-(--control-height-sm) rounded-sm border border-transparent px-5 text-secondary tabular-nums aria-pressed:border-accent aria-pressed:bg-surface-3 aria-pressed:text-primary',
  },
});

/**
 * An on/off switch: a button with role="switch" and aria-checked, its knob, and its label. On and off are said by the
 * knob's place and the label, not by color alone.
 */
export const toggleSwitch = tv({
  slots: {
    root: 'group inline-flex min-h-(--control-height) items-center gap-4 text-secondary',
    knob: 'relative h-(--switch-height) w-(--switch-width) flex-none rounded-pill bg-surface-3 transition-colors group-aria-checked:bg-accent after:absolute after:top-(--switch-gap) after:left-(--switch-gap) after:size-(--switch-knob) after:rounded-pill after:bg-muted after:transition-transform group-aria-checked:after:translate-x-(--switch-travel) group-aria-checked:after:bg-on-accent',
  },
});
