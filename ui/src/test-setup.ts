// What the tests' browser stand-in (jsdom) lacks and the app uses: a ResizeObserver, a canvas that can be asked for
// a context (jsdom has none to give, and logs an error when asked), and object URLs for files opened in the browser.
class NoResizeObserver {
  observe(): void {
    // nothing resizes in tests
  }
  disconnect(): void {
    // nothing to stop
  }
}

globalThis.ResizeObserver ??= NoResizeObserver as unknown as typeof ResizeObserver;
HTMLCanvasElement.prototype.getContext = (() =>
  null) as typeof HTMLCanvasElement.prototype.getContext;

let objectUrls = 0;
URL.createObjectURL ??= () => `blob:test/${++objectUrls}`;
