import { tv } from 'tailwind-variants/lite';

/** The player: the video, its overlay and clock, the seek bar and the controls. */
export const playerStyles = tv({
  slots: {
    screen: 'relative overflow-hidden rounded-lg',
    video: 'block size-full',
    canvas: 'pointer-events-none absolute inset-0 size-full',
    time: 'absolute bottom-4 left-4 rounded-md px-3 py-1 text-xs tabular-nums',
    seekRow: 'relative mt-4 h-(--seek-height)',
    marks: 'pointer-events-none absolute inset-x-4 inset-y-0',
    seek: 'absolute inset-0 w-full cursor-pointer accent-accent',
    controls: 'mt-4 flex flex-wrap items-center gap-4',
    icon: 'size-(--icon-size-sm)',
    hint: 'ml-auto text-xs text-muted',
  },
});
