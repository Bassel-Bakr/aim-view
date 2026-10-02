import { Directive, input } from '@angular/core';

/** What a button is for: an ordinary action, or the one main action on a page. */
export type ButtonIntent = 'normal' | 'primary';

/** The app's button (themes/controls.scss: `.button`), its variant set by a typed input. */
@Directive({
  selector: 'button[appButton], a[appButton]',
  host: { class: 'button', '[attr.data-intent]': 'intent()' },
})
export class Button {
  readonly intent = input<ButtonIntent>('normal');
}
