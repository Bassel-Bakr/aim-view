import { tv } from 'tailwind-variants/lite';

/** Upload: its button, what the last upload did, and the zone shown while files are dragged over the page. */
export const uploadStyles = tv({
  slots: {
    icon: 'size-(--icon-size-sm)',
    note: 'truncate text-xs text-secondary',
    failed: 'truncate text-xs text-warning',
    dropZone: 'fixed inset-0 z-10 grid place-items-center bg-backdrop p-13',
    dropBox:
      'flex size-full flex-col items-center justify-center gap-3 rounded-lg border-(length:--line-width-strong) border-dashed border-accent text-secondary',
    dropTitle: 'text-xl font-strong text-primary',
  },
});
