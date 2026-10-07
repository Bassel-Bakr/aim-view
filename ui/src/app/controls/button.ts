/**
 * The `appButton` directive: gives a button or link the shared button's class and its intent as a
 * data attribute, which themes/controls.scss styles.
 */

import { Directive, input } from '@angular/core';

/** What a button is for: an ordinary action, or the one main action on a page. */
export type ButtonIntent = 'normal' | 'primary';

/** The app's button (themes/controls.scss: `.button`), its variant set by a typed input. */
@Directive({
  selector: 'button[appButton], a[appButton]',
  host: { class: 'button', '[attr.data-intent]': 'intent()' },
})
export class Button {
  /** What the button is for, set as data-intent. */
  readonly intent = input<ButtonIntent>('normal');
}
