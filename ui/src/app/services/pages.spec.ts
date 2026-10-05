import { TestBed } from '@angular/core/testing';
import { serverMode } from '../fake-api';
import { Library } from './library';
import { Pages } from './pages';

/** A media query that says the screen is narrow, as a phone's (jsdom has no matchMedia). */
function narrowScreen(): MediaQueryList {
  return {
    matches: true,
    addEventListener: () => undefined,
  } as unknown as MediaQueryList;
}

describe('the recordings list', () => {
  afterEach(() => {
    Reflect.deleteProperty(window, 'matchMedia');
    document.documentElement.style.removeProperty('--narrow-width');
    localStorage.removeItem('aimview-list');
    history.replaceState(null, '', '/');
  });

  it('opens and closes as a column on a wide screen, and the choice is kept', () => {
    TestBed.configureTestingModule({ providers: serverMode() });
    const pages = TestBed.inject(Pages);
    expect(pages.listOpen()).toBe(true);
    pages.toggleList();
    expect(pages.listOpen()).toBe(false);
    expect(localStorage.getItem('aimview-list')).toBe('closed');
  });

  it('is a drawer on a narrow screen: open with no recording, closing when one is picked', () => {
    document.documentElement.style.setProperty('--narrow-width', '767px');
    Object.defineProperty(window, 'matchMedia', { configurable: true, value: narrowScreen });
    TestBed.configureTestingModule({ providers: serverMode() });
    const pages = TestBed.inject(Pages);
    TestBed.tick();
    expect(pages.narrow()).toBe(true);
    expect(pages.listOpen()).toBe(true);
    TestBed.inject(Library).selectedId.set('Air/Air - 1 - 2026.10.01-16.23.03.mp4');
    TestBed.tick();
    expect(pages.listOpen()).toBe(false);
    pages.toggleList();
    expect(pages.listOpen()).toBe(true);
    expect(localStorage.getItem('aimview-list')).toBeNull();
  });
});
