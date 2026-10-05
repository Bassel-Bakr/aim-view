import { DestroyRef } from '@angular/core';

/**
 * Listens to an element's event outside the template, until the view that asked goes away. In this app (no zone.js)
 * a template's listener marks its view for change detection on every event, so one on an event that fires many times
 * a second (the pointer moving, the wheel, a slider being dragged) runs change detection that often. Here only the
 * signals the handler changes ask for it.
 */
export function listenQuietly<K extends keyof GlobalEventHandlersEventMap>(
  element: Element,
  type: K,
  handler: (event: GlobalEventHandlersEventMap[K]) => void,
  destroyRef: DestroyRef,
  options?: AddEventListenerOptions,
): void {
  const listener = handler as EventListener;
  element.addEventListener(type, listener, options);
  destroyRef.onDestroy(() => element.removeEventListener(type, listener, options));
}
