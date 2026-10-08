import { TestBed } from '@angular/core/testing';
import { Theme, THEME_STORAGE_KEY } from './theme';

describe('Theme', () => {
  afterEach(() => {
    localStorage.removeItem(THEME_STORAGE_KEY);
    delete document.documentElement.dataset['theme'];
  });

  it('starts from the stored choice and puts it on the page', () => {
    localStorage.setItem(THEME_STORAGE_KEY, 'light');
    const theme = TestBed.inject(Theme);
    expect(theme.choice()).toBe('light');
    expect(theme.scheme()).toBe('light');
    expect(document.documentElement.dataset['theme']).toBe('light');
  });

  it('keeps a choice, and System removes it so the system decides', () => {
    const theme = TestBed.inject(Theme);
    theme.choose('dark');
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe('dark');
    expect(document.documentElement.dataset['theme']).toBe('dark');
    theme.choose('system');
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBeNull();
    expect(document.documentElement.dataset['theme']).toBeUndefined();
  });

  it('follows the choice in its scheme', () => {
    const theme = TestBed.inject(Theme);
    theme.choose('light');
    expect(theme.scheme()).toBe('light');
    theme.choose('dark');
    expect(theme.scheme()).toBe('dark');
  });
});
