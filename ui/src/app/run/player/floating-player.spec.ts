import { EnvironmentInjector } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Playback } from '../playback';
import { FloatingPlayer } from './floating-player';

/** The view watch's callback, kept so a test can say how much of the player shows. */
let reportShare: (share: number) => void = () => undefined;

class FakeObserver {
  constructor(callback: IntersectionObserverCallback) {
    reportShare = (share) =>
      callback(
        [{ intersectionRatio: share } as IntersectionObserverEntry],
        this as unknown as IntersectionObserver,
      );
  }
  observe(): void {
    // the test reports the share itself
  }
  disconnect(): void {
    // nothing to stop
  }
}

describe('the floating player', () => {
  it('docks while playing out of view, stays docked if paused there, and goes back in view or when closed', () => {
    const original = globalThis.IntersectionObserver;
    globalThis.IntersectionObserver = FakeObserver as unknown as typeof IntersectionObserver;
    const shown = new Set<HTMLElement>();
    HTMLElement.prototype.showPopover = function (this: HTMLElement) {
      shown.add(this);
    };
    HTMLElement.prototype.hidePopover = function (this: HTMLElement) {
      shown.delete(this);
    };
    const playback = TestBed.inject(Playback);
    const floating = new FloatingPlayer(playback, () => false, TestBed.inject(EnvironmentInjector));
    const [frame, screen] = [document.createElement('div'), document.createElement('div')];
    const video = document.createElement('video');
    playback.attach(video);
    floating.attach({ frame, screen, video, redraw: () => undefined });
    const settle = () => TestBed.tick();

    reportShare(0);
    settle();
    expect(floating.docked()).toBe(false);
    playback.paused.set(false);
    settle();
    expect(floating.docked()).toBe(true);
    expect(shown.has(screen)).toBe(true);
    playback.paused.set(true);
    settle();
    expect(floating.docked()).toBe(true);
    reportShare(1);
    settle();
    expect(floating.docked()).toBe(false);
    reportShare(0);
    playback.paused.set(false);
    settle();
    floating.close();
    settle();
    expect(floating.docked()).toBe(false);
    expect(shown.has(screen)).toBe(false);
    globalThis.IntersectionObserver = original;
  });
});
