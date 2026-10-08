/**
 * The app's one tooltip: a popover (themes/controls.scss `.tooltip`) showing the `data-tooltip` words of the part
 * under the mouse or the keyboard's focus. One element and a few listeners on the document serve every part, outside
 * Angular, so a tooltip costs no component, no directive and no change detection. While it shows, the part names it
 * as its description (aria-describedby) and as its anchor, which the style places it by. In: data-tooltip attributes
 * in any template. Out: the tooltip; started by app.config.ts.
 */

import { DestroyRef, DOCUMENT, inject, Service } from '@angular/core';

/** The attribute a part keeps its tooltip's words in. */
export const TOOLTIP_ATTRIBUTE = 'data-tooltip';
/** The anchor name the shown part takes; the tooltip's style (position-anchor) names the same. */
const ANCHOR_NAME = '--tooltip-anchor';
/** The tooltip's id, which the shown part's aria-describedby names. */
const TOOLTIP_ID = 'app-tooltip';
/** Where the browser places the tooltip by the part itself (CSS anchor positioning). */
const ANCHORS_SUPPORTED = typeof CSS !== 'undefined' && CSS.supports('position-area: top');

/** Shows each part's data-tooltip words in one popover, for the mouse and the keyboard. */
@Service()
export class Tooltips {
  /** The page the listeners and the tooltip are in. */
  private readonly document = inject(DOCUMENT);
  /** The one tooltip element, moved to whichever part shows it. */
  private readonly tooltip = this.createTooltip();
  /** The part whose tooltip shows, or null. */
  private shown: HTMLElement | null = null;
  /** The shown part's own aria-describedby, put back when the tooltip goes. */
  private describedBefore: string | null = null;
  /** Ends the listening when the app (or a test's injector) ends. */
  private readonly listening = new AbortController();

  /** Stops the listeners with the injector. */
  constructor() {
    inject(DestroyRef).onDestroy(() => this.listening.abort());
  }

  /** Listens on the document: a mouse resting on a part or the keyboard's focus shows its tooltip. */
  start(): void {
    const doc = this.document;
    const signal = this.listening.signal;
    doc.addEventListener('pointerover', (event) => this.pointerOver(event), { signal });
    doc.addEventListener('pointerout', (event) => this.pointerOut(event), { signal });
    doc.addEventListener('focusin', (event) => this.focusIn(event), { signal });
    doc.addEventListener('focusout', () => this.hide(), { signal });
    doc.addEventListener('pointerdown', () => this.hide(), { signal, capture: true });
    doc.addEventListener('scroll', () => this.hide(), { signal, capture: true, passive: true });
    doc.addEventListener('keydown', (event) => this.keyDown(event), { signal });
  }

  /** Escape hides the tooltip. */
  private keyDown(event: KeyboardEvent): void {
    if (event.key === 'Escape') this.hide();
  }

  /** A mouse over a part shows its tooltip; a touch does not (a tap is a click there). */
  private pointerOver(event: PointerEvent): void {
    if (event.pointerType !== 'mouse') return;
    const part = this.partAt(event.target);
    if (part && part !== this.shown) this.show(part);
  }

  /** The mouse leaving the shown part (not into one of its children) hides its tooltip. */
  private pointerOut(event: PointerEvent): void {
    if (this.shown && !this.shown.contains(event.relatedTarget as Node | null)) this.hide();
  }

  /** The keyboard's focus on a part shows its tooltip; a click's focus does not. */
  private focusIn(event: FocusEvent): void {
    const part = this.partAt(event.target);
    if (part && part.matches(':focus-visible')) this.show(part);
  }

  /** The part with a tooltip at or around an event's target. */
  private partAt(target: EventTarget | null): HTMLElement | null {
    return target instanceof Element ? target.closest<HTMLElement>(`[${TOOLTIP_ATTRIBUTE}]`) : null;
  }

  /** Shows a part's tooltip, inside its open dialog when it has one (the rest of the page is inert then). */
  private show(part: HTMLElement): void {
    const words = part.getAttribute(TOOLTIP_ATTRIBUTE);
    this.hide();
    if (!words) return;
    this.tooltip.textContent = words;
    (part.closest('dialog[open]') ?? this.document.body).append(this.tooltip);
    part.style.setProperty('anchor-name', ANCHOR_NAME);
    this.describedBefore = part.getAttribute('aria-describedby');
    part.setAttribute(
      'aria-describedby',
      [this.describedBefore, TOOLTIP_ID].filter(Boolean).join(' '),
    );
    this.shown = part;
    this.tooltip.showPopover();
    if (!ANCHORS_SUPPORTED) this.place(part);
  }

  /** Hides the tooltip and gives the part back its own description. */
  private hide(): void {
    const part = this.shown;
    if (!part) return;
    this.shown = null;
    this.tooltip.hidePopover();
    part.style.removeProperty('anchor-name');
    if (this.describedBefore === null) part.removeAttribute('aria-describedby');
    else part.setAttribute('aria-describedby', this.describedBefore);
  }

  /**
   * Places the tooltip where the browser cannot by anchor: above the part, below it when there is no room, kept
   * inside the window's width. In CSS pixels, from the viewport's corner; the margins are the style's gap.
   */
  private place(part: HTMLElement): void {
    const box = part.getBoundingClientRect();
    const tip = this.tooltip;
    const gap_px = parseFloat(getComputedStyle(tip).marginTop) || 0;
    const above_px = box.top - tip.offsetHeight - 2 * gap_px;
    const left_px = box.left + box.width / 2 - tip.offsetWidth / 2;
    tip.style.top = `${above_px >= 0 ? above_px : box.bottom}px`;
    tip.style.left = `${Math.max(0, Math.min(left_px, innerWidth - tip.offsetWidth))}px`;
  }

  /** The tooltip element: a manual popover (it shows and hides only when told), read as a tooltip. */
  private createTooltip(): HTMLElement {
    const tip = this.document.createElement('div');
    tip.id = TOOLTIP_ID;
    tip.className = 'tooltip';
    tip.setAttribute('popover', 'manual');
    tip.setAttribute('role', 'tooltip');
    return tip;
  }
}
