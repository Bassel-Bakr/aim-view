/**
 * What the tests' browser stand-in (jsdom) lacks and the app uses: a ResizeObserver, an
 * IntersectionObserver (nothing scrolls into view, so what loads on viewport stays unloaded), a
 * canvas that can be asked for a context (jsdom has none to give, and logs an error when asked),
 * object URLs for files opened in the browser, and scrolling an element into view (the recordings
 * list keeps the open recording's row in view). The UI's tests run it before each spec file.
 */

/** A ResizeObserver or IntersectionObserver that never calls back. */
class NoObserver {
  /** Watches nothing. */
  observe(): void {
    // nothing resizes or scrolls in tests
  }
  /** Stops watching nothing. */
  unobserve(): void {
    // nothing to stop
  }
  /** Stops watching nothing. */
  disconnect(): void {
    // nothing to stop
  }
}

globalThis.ResizeObserver ??= NoObserver as unknown as typeof ResizeObserver;
globalThis.IntersectionObserver ??= NoObserver as unknown as typeof IntersectionObserver;
Element.prototype.scrollIntoView ??= () => undefined;
HTMLCanvasElement.prototype.getContext = (() =>
  null) as typeof HTMLCanvasElement.prototype.getContext;

/** How many object URLs the tests have made, so each one is new. */
let objectUrls = 0;
URL.createObjectURL ??= () => `blob:test/${++objectUrls}`;
