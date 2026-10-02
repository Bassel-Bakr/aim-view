import { Directive, input } from '@angular/core';

/** A badge's tone: neutral, or good (reviewed, done). */
export type BadgeTone = 'neutral' | 'good';

/** A status in words (themes/controls.scss: `.badge`), its tone set by a typed input. */
@Directive({
  selector: '[appBadge]',
  host: { class: 'badge', '[attr.data-tone]': 'tone()' },
})
export class Badge {
  readonly tone = input<BadgeTone>('neutral');
}
