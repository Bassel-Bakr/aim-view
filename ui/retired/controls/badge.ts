/**
 * The `appBadge` directive: gives an element the shared badge's class and its tone as a data
 * attribute, which themes/controls.scss styles.
 */

import { Directive, input } from '@angular/core';

/** A badge's tone: neutral, or good (reviewed, done). */
export type BadgeTone = 'neutral' | 'good';

/** A status in words (themes/controls.scss: `.badge`), its tone set by a typed input. */
@Directive({
  selector: '[appBadge]',
  host: { class: 'badge', '[attr.data-tone]': 'tone()' },
})
export class Badge {
  /** The badge's tone, set as data-tone. */
  readonly tone = input<BadgeTone>('neutral');
}
