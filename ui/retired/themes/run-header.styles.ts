import { tv } from 'tailwind-variants/lite';

/** The run's header: its title, score line and source. */
export const runHeaderStyles = tv({
  slots: {
    title: 'flex flex-wrap items-center gap-4',
    sub: 'mt-2 text-muted',
    detail: 'mt-1 text-xs text-muted',
  },
});
