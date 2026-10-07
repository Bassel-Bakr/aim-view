/**
 * The player's video (with its overlay) floating over the page, as YouTube's does: docked in a
 * corner of the page while it plays and the player is scrolled out of view (so a kill picked in the
 * report below shows at once), and moved into the browser's own picture-in-picture window. The
 * docked video goes into the page's top layer as a popover, as the player's filled window does: the
 * main area measures its width, which would hold a fixed layer inside it.
 *
 * In: the player's elements (player.ts), whether the video plays (Playback) and whether the player
 * is in view. Out: the video's place on the page, and the signals the player's template reads.
 */

import { DestroyRef, effect, Injector, signal, untracked } from '@angular/core';
import { Playback } from '../playback';

/**
 * The browser's picture-in-picture window for a whole element (Chrome and Edge's Document
 * Picture-in-Picture).
 */
interface DocumentPictureInPicture {
  /** Opens the window at the given size; resolves to the window, whose document the page fills. */
  requestWindow(options: PictureInPictureSize): Promise<Window>;
}

/** The size the picture-in-picture window opens at, in CSS pixels. */
interface PictureInPictureSize {
  /** The window's width, in CSS pixels. */
  width: number;
  /** The window's height, in CSS pixels. */
  height: number;
}

/** The window, with the document picture-in-picture where the browser has it. */
interface WindowWithPictureInPicture {
  /** Chrome and Edge's Document Picture-in-Picture; undefined in other browsers. */
  documentPictureInPicture?: DocumentPictureInPicture;
}

/** The parts of the player the floating video moves, and how it redraws the overlay. */
export interface FloatingParts {
  /**
   * The player's frame, which stays in the page: its share in view decides when the video docks.
   */
  frame: HTMLElement;
  /** The box with the video and its overlay, which docks or moves into the window. */
  screen: HTMLElement;
  /**
   * The video, which goes alone into the browser's picture-in-picture where it cannot take the box.
   */
  video: HTMLVideoElement;
  /** Redraws the overlay (the canvas changed size). */
  redraw: () => void;
}

/** Share of the player that must show for it to count as in view. */
const IN_VIEW_SHARE = 0.25;

/**
 * Docks the player's video in the page's corner while it plays out of view, and moves it into the
 * browser's picture-in-picture window and back. The player makes one and attaches it once its
 * elements are on the page.
 */
export class FloatingPlayer {
  /** The player is in view (a quarter of it or more). */
  private readonly inView = signal(true);
  /** The docked video was closed: it stays in place until the player is back in view. */
  private readonly closed = signal(false);
  /** Whether the video is docked; written here only, read outside through `docked`. */
  private readonly docking = signal(false);
  /** The video floats in the page's corner. */
  readonly docked = this.docking.asReadonly();
  /** The video is in the browser's picture-in-picture window. */
  readonly inWindow = signal(false);
  /**
   * The player's height while its video is away, in CSS pixels, so the page does not jump; null
   * while it is in place.
   */
  readonly placeholder = signal<number | null>(null);
  /** The browser can float the video in a window of its own. */
  readonly canWindow = documentPip() !== undefined || document.pictureInPictureEnabled === true;
  /** The document picture-in-picture window while the video is in it, else null. */
  private pipWindow: Window | null = null;
  /** The player's elements, once attached. */
  private parts: FloatingParts | null = null;

  /**
   * Takes the video's playback, whether the player fills the screen or the window (it never docks
   * then), and the player's injector, for the effect and the clean-up on destroy.
   */
  constructor(
    private readonly playback: Playback,
    private readonly full: () => boolean,
    private readonly injector: Injector,
  ) {}

  /** Starts watching the player, once its parts are on the page. */
  attach(parts: FloatingParts): void {
    this.parts = parts;
    const watch = new IntersectionObserver(
      ([entry]) => this.inView.set(entry.intersectionRatio >= IN_VIEW_SHARE),
      { threshold: [IN_VIEW_SHARE] },
    );
    watch.observe(parts.frame);
    effect(() => this.follow(), { injector: this.injector });
    this.injector.get(DestroyRef).onDestroy(() => {
      watch.disconnect();
      this.pipWindow?.close();
    });
  }

  /**
   * Docks while playing out of view (and stays docked if paused there); back in place in view, in
   * full screen, in the window, or once closed.
   */
  private follow(): void {
    const inView = this.inView();
    const playing = !this.playback.paused();
    const stay = !inView && !this.closed() && !this.full() && !this.inWindow();
    untracked(() => {
      if (inView) this.closed.set(false);
      if (stay && (playing || this.docking())) this.dock();
      else if (!stay && this.docking()) this.undock();
    });
  }

  /**
   * Puts the video's box in the page's top layer (docked in the corner by its styles), keeping its
   * height in the page; does nothing when already docked or where the browser has no popovers.
   */
  private dock(): void {
    const parts = this.parts;
    if (this.docking() || !parts || !('showPopover' in parts.screen)) return;
    this.placeholder.set(parts.screen.getBoundingClientRect().height);
    parts.screen.setAttribute('popover', 'manual');
    parts.screen.showPopover();
    this.docking.set(true);
    parts.redraw();
  }

  /** Puts the docked video's box back in the player. */
  private undock(): void {
    const parts = this.parts;
    if (!parts) return;
    parts.screen.hidePopover();
    parts.screen.removeAttribute('popover');
    this.docking.set(false);
    this.placeholder.set(null);
    parts.redraw();
  }

  /** The docked video's close: it stops, and goes back in place. */
  close(): void {
    this.playback.pause();
    this.closed.set(true);
  }

  /** Scrolls the page back to the player, where the video goes back in place. */
  backToPlayer(): void {
    this.parts?.frame.scrollIntoView({ behavior: 'smooth', block: 'center' });
  }

  /**
   * Moves the video, with its overlay, into the browser's picture-in-picture window, or brings it
   * back. Where the browser cannot move a whole element there (Firefox, Safari), the video alone
   * goes, without the overlay.
   */
  async toggleWindow(): Promise<void> {
    if (this.pipWindow) {
      this.pipWindow.close();
      return;
    }
    const parts = this.parts;
    const pipApi = documentPip();
    if (!parts) return;
    if (!pipApi) {
      await parts.video.requestPictureInPicture().catch(() => undefined);
      return;
    }
    if (this.docking()) this.undock();
    const box = parts.screen.getBoundingClientRect();
    const pip = await pipApi.requestWindow({
      width: Math.round(box.width),
      height: Math.round(box.height),
    });
    this.moveInto(pip, parts);
  }

  /**
   * Moves the video's box into the opened picture-in-picture window with the page's styles, and
   * puts it back where it was when the window closes.
   */
  private moveInto(pip: Window, parts: FloatingParts): void {
    const screen = parts.screen;
    const [home, after] = [screen.parentElement, screen.nextSibling];
    copyStyles(pip.document);
    this.placeholder.set(screen.getBoundingClientRect().height);
    pip.document.body.classList.add('pip-body');
    pip.document.body.append(screen);
    this.pipWindow = pip;
    this.inWindow.set(true);
    pip.addEventListener('resize', () => parts.redraw());
    pip.addEventListener('pagehide', () => {
      home?.insertBefore(screen, after);
      this.pipWindow = null;
      this.inWindow.set(false);
      this.placeholder.set(null);
      parts.redraw();
    });
    parts.redraw();
  }
}

/** The browser's Document Picture-in-Picture, or undefined where it has none (Firefox, Safari). */
function documentPip(): DocumentPictureInPicture | undefined {
  return (window as WindowWithPictureInPicture).documentPictureInPicture;
}

/**
 * The page's styles, copied into the picture-in-picture window, so the video and its overlay look
 * the same there.
 */
function copyStyles(target: Document): void {
  for (const sheet of [...document.styleSheets]) {
    try {
      const style = target.createElement('style');
      style.textContent = [...sheet.cssRules].map((rule) => rule.cssText).join('\n');
      target.head.append(style);
    } catch {
      // a sheet from another origin cannot be read: linked instead
      if (!sheet.href) continue;
      const link = target.createElement('link');
      link.rel = 'stylesheet';
      link.href = sheet.href;
      target.head.append(link);
    }
  }
}
