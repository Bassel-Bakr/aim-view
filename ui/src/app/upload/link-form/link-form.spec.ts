import { signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { LinkInfo } from '../../api';
import { RecordingSource } from '../../platform/recording-source';
import { Library } from '../../services/library';
import { LinkForm, qualityLabel } from './link-form';

const LINK = 'https://www.youtube.com/watch?v=abc';
const ADDED = 'uploads/A run - 2026.10.04-12.00.00.mp4';
const QUALITIES: LinkInfo = {
  title: 'A run',
  duration: 42,
  formats: [
    { id: '400', width: 2560, height: 1440, fps: 59.94, codec: 'AV1', size: 412e6 },
    { id: '136', width: 1280, height: 720, fps: 30, codec: 'H.264', size: null },
  ],
};

/** A link added: the link, and the quality chosen. */
type AddedLink = [string, string | null];

/** A recording source that answers links: what linkInfo answers (or the error it throws), and the links added. */
class LinkSource {
  readonly recordings = signal([]);
  readonly linkServer = null;
  answer: LinkInfo | Error = QUALITIES;
  readonly added: AddedLink[] = [];
  lasting(): boolean {
    return true;
  }
  async linkInfo(): Promise<LinkInfo> {
    if (this.answer instanceof Error) throw this.answer;
    return this.answer;
  }
  async addLink(url: string, format: string | null): Promise<string> {
    this.added.push([url, format]);
    return ADDED;
  }
}

async function render(source: LinkSource): Promise<ComponentFixture<LinkForm>> {
  // jsdom's dialog cannot close
  HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
    this.removeAttribute('open');
  };
  TestBed.configureTestingModule({ providers: [{ provide: RecordingSource, useValue: source }] });
  const fixture = TestBed.createComponent(LinkForm);
  await fixture.whenStable();
  return fixture;
}

const el = (fixture: ComponentFixture<LinkForm>) => fixture.nativeElement as HTMLElement;

/** Types or pastes a link into the field. */
async function enter(
  fixture: ComponentFixture<LinkForm>,
  url: string,
  paste: boolean,
): Promise<void> {
  const input = el(fixture).querySelector('input[aria-label=Link]') as HTMLInputElement;
  input.value = url;
  input.dispatchEvent(new Event('input'));
  if (paste) input.dispatchEvent(new Event('paste'));
  await new Promise((resolve) => setTimeout(resolve));
  await fixture.whenStable();
}

async function submit(fixture: ComponentFixture<LinkForm>): Promise<void> {
  el(fixture).querySelector('form')?.dispatchEvent(new Event('submit'));
  await new Promise((resolve) => setTimeout(resolve));
  await fixture.whenStable();
}

describe('qualityLabel', () => {
  it('names a quality by what is known of it', () => {
    expect(qualityLabel(QUALITIES.formats[0])).toBe('2560x1440 · 60 fps · AV1 · 412 MB');
    expect(qualityLabel(QUALITIES.formats[1])).toBe('1280x720 · 30 fps · H.264');
    expect(qualityLabel({ ...QUALITIES.formats[1], size: 475961 })).toBe(
      '1280x720 · 30 fps · H.264 · under 1 MB',
    );
    expect(
      qualityLabel({ id: 'hls', width: null, height: 1080, fps: null, codec: null, size: null }),
    ).toBe('1080p');
  });
});

describe('LinkForm', () => {
  afterEach(() => history.replaceState(null, '', '/'));

  it('reads a pasted link, offers its qualities with the best chosen, and adds the one picked', async () => {
    const source = new LinkSource();
    const fixture = await render(source);
    await enter(fixture, LINK, true);
    const select = el(fixture).querySelector('select') as HTMLSelectElement;
    expect([...select.options].map((option) => option.textContent?.trim())).toEqual([
      '2560x1440 · 60 fps · AV1 · 412 MB (best)',
      '1280x720 · 30 fps · H.264',
    ]);
    expect(select.value).toBe('400');
    expect(el(fixture).textContent).toContain(
      'The sharpest source and the highest frame rate review best',
    );
    select.value = '136';
    select.dispatchEvent(new Event('input'));
    select.dispatchEvent(new Event('change'));
    await submit(fixture);
    expect(source.added).toEqual([[LINK, '136']]);
    expect(TestBed.inject(Library).selectedId()).toBe(ADDED);
  });

  it('adds a link with nothing to choose at once', async () => {
    const source = new LinkSource();
    source.answer = { title: 'clip.mp4', duration: null, formats: [] };
    const fixture = await render(source);
    await enter(fixture, 'https://cdn.example.com/clip.mp4', false);
    await submit(fixture);
    expect(el(fixture).querySelector('select')).toBeNull();
    expect(source.added).toEqual([['https://cdn.example.com/clip.mp4', null]]);
  });

  it('with qualities to choose from, Add first shows them', async () => {
    const source = new LinkSource();
    const fixture = await render(source);
    await enter(fixture, LINK, false);
    await submit(fixture);
    expect(el(fixture).querySelector('select')).not.toBeNull();
    expect(source.added).toEqual([]);
    await submit(fixture);
    expect(source.added).toEqual([[LINK, '400']]);
  });

  it('says why a link cannot be read, and adds nothing', async () => {
    const source = new LinkSource();
    source.answer = new Error('yt-dlp cannot read this link: Private video');
    const fixture = await render(source);
    await enter(fixture, LINK, true);
    expect(el(fixture).querySelector('[role=alert]')?.textContent).toContain('Private video');
    await submit(fixture);
    expect(source.added).toEqual([]);
  });

  it('takes only web links', async () => {
    const fixture = await render(new LinkSource());
    await enter(fixture, 'youtube.com/watch?v=abc', false);
    const add = el(fixture).querySelector('button[type=submit]') as HTMLButtonElement;
    expect(add.disabled).toBe(true);
  });
});
