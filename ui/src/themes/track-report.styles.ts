import { tv } from 'tailwind-variants/lite';

/** A tracking run's report: its cards, how the crosshair followed the bot, and what would raise the accuracy. */
export const trackReportStyles = tv({
  slots: {
    cards: 'mt-2 grid grid-cols-(--cards-columns) gap-4',
    why: 'text-xs text-muted',
    whatIf: 'w-full',
    gain: 'font-strong text-value',
    how: 'text-left whitespace-normal text-muted',
  },
});
