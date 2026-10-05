import { HttpRequest } from '@angular/common/http';
import { TestBed } from '@angular/core/testing';
import { ModelList } from '../api';
import { answer, ApiRoutes, serverMode } from '../fake-api';
import { ModelPanel } from './model-panel';

/** The review server's models: two, either of which can be picked. */
function fakeModels(): ApiRoutes {
  let chosen = 'full_v3';
  const list = (): ModelList => ({
    chosen,
    device: 'cuda',
    speed: '',
    checked_on: '',
    checks: [],
    models: [
      { name: 'full_v3', label: 'full_v3', available: true, default: true },
      { name: 'small_v13', label: 'small_v13', available: true },
    ],
  });
  return {
    '/api/models': list,
    '/api/model': (req: HttpRequest<unknown>) => {
      chosen = req.params.get('name') ?? chosen;
      return list();
    },
    '/api/report': null,
  };
}

describe('the models dialog', () => {
  it('keeps its body from one opening to the next, and clears the last switch’s word', async () => {
    // jsdom's dialog cannot open as a modal or close
    HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
      this.setAttribute('open', '');
    };
    HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
      this.removeAttribute('open');
    };
    TestBed.configureTestingModule({ providers: serverMode() });
    const fixture = TestBed.createComponent(ModelPanel);
    const routes = fakeModels();
    const settle = async () => {
      await answer(routes);
      await fixture.whenStable();
    };
    await settle();
    const el = fixture.nativeElement as HTMLElement;
    const button = (text: string) =>
      [...el.querySelectorAll('button')].find((b) =>
        b.textContent?.trim().startsWith(text),
      ) as HTMLButtonElement;
    (el.querySelector('.trigger') as HTMLButtonElement).click();
    await settle();
    const body = el.querySelector('app-model-choice');
    button('Use ').click();
    await settle();
    expect(el.querySelector('.status')?.textContent).toContain('Now using');
    button('Close').click();
    await settle();
    (el.querySelector('.trigger') as HTMLButtonElement).click();
    await settle();
    expect(el.querySelector('app-model-choice')).toBe(body);
    expect(el.querySelector('.status')?.textContent?.trim()).toBe('');
  });
});
