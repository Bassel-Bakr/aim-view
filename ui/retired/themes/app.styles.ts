import { tv } from 'tailwind-variants/lite';

/** The app's frame: the top bar, the main area and the empty page. */
export const appStyles = tv({
  slots: {
    top: 'flex items-center gap-7 border-b border-border bg-surface-1 px-9',
    brand: 'flex items-center gap-4 text-lg font-strong',
    logo: 'size-(--icon-size) text-accent',
    // at the right end: the Upload button last, its status before it
    upload: 'flex min-w-0 flex-1 flex-row-reverse items-center gap-5',
    main: 'min-w-0 overflow-y-auto px-13 py-11',
    empty: 'mt-(--empty-offset) flex max-w-(--empty-width) flex-col gap-4 text-secondary',
    emptyTitle: 'text-primary',
  },
});
