import {
  afterEveryRender,
  ApplicationRef,
  createEnvironmentInjector,
  DestroyRef,
  EnvironmentInjector,
} from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { listenQuietly } from './listen-quietly';

describe('listenQuietly', () => {
  it('hands each event to its handler with no render, and stops when its view goes', async () => {
    const root = TestBed.inject(EnvironmentInjector);
    const app = TestBed.inject(ApplicationRef);
    const view = createEnvironmentInjector([], root);
    const box = document.createElement('div');
    const seen: number[] = [];
    listenQuietly(box, 'pointermove', (event) => seen.push(event.clientX), view.get(DestroyRef));
    let renders = 0;
    afterEveryRender(() => (renders += 1), { injector: root });
    await app.whenStable();
    const before = renders;
    box.dispatchEvent(new MouseEvent('pointermove', { clientX: 4 }));
    box.dispatchEvent(new MouseEvent('pointermove', { clientX: 8 }));
    await new Promise((resolve) => setTimeout(resolve));
    await app.whenStable();
    expect(seen).toEqual([4, 8]);
    expect(renders).toBe(before);
    view.destroy();
    box.dispatchEvent(new MouseEvent('pointermove', { clientX: 12 }));
    expect(seen).toEqual([4, 8]);
  });
});
